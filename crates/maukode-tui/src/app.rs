use crate::event::AppEvent;
use maukode_core::{
    AppConfig, CheckpointManager, ContextManager, PermissionManager, PermissionMode,
    ProviderConfig, ProviderKind, RunnerConfig, Session, SessionRunner, SessionSummary,
    SqliteSessionStore,
};
use maukode_protocol::{
    ContentPart, FinishReason, LLMEvent, Message, PermissionDecision, PermissionRequest, Role,
    SessionId, TokenUsage,
};
use maukode_server::ServerClient;
use maukode_tools::{
    create_default_registry, create_default_registry_with_jail, PathJail, ToolRegistry,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    ToolCall {
        name: String,
        id: String,
        args: String,
    },
    ToolResult {
        name: String,
        id: String,
        is_error: bool,
    },
    Reasoning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatItem {
    pub role: ChatRole,
    pub content: String,
}

// Keep streamed chunks in one item so a burst does not create a widget per token.
pub(crate) fn append_stream_delta(items: &mut Vec<ChatItem>, role: ChatRole, text: String) {
    if let Some(last) = items.last_mut() {
        if std::mem::discriminant(&last.role) == std::mem::discriminant(&role) {
            last.content.push_str(&text);
            return;
        }
    }
    items.push(ChatItem {
        role,
        content: text,
    });
}

fn remote_turn_result(event: &LLMEvent) -> Option<Result<(), String>> {
    match event {
        LLMEvent::Error { message } => Some(Err(message.clone())),
        LLMEvent::Finish { reason } => match reason {
            FinishReason::ToolUse => None,
            FinishReason::Error => Some(Err("Provider ended the turn with an error".into())),
            _ => Some(Ok(())),
        },
        _ => None,
    }
}

fn finish_tool_call(items: &mut [ChatItem], call: &maukode_protocol::ToolCall) {
    if let Some(item) = items
        .iter_mut()
        .rev()
        .find(|item| matches!(&item.role, ChatRole::ToolCall { id, .. } if id == &call.id))
    {
        if let ChatRole::ToolCall { name, args, .. } = &mut item.role {
            *name = call.name.clone();
            *args = call.arguments.to_string();
        }
    }
}

pub fn chat_items_from_messages(messages: &[Message]) -> Vec<ChatItem> {
    let mut items = Vec::new();
    for message in messages {
        match message.role {
            Role::User => {
                for part in &message.parts {
                    match part {
                        ContentPart::Text { text } if !text.is_empty() => items.push(ChatItem {
                            role: ChatRole::User,
                            content: text.clone(),
                        }),
                        ContentPart::ToolResult(result) => items.push(ChatItem {
                            role: ChatRole::ToolResult {
                                name: result.tool_name.clone(),
                                id: result.tool_call_id.clone(),
                                is_error: result.is_error,
                            },
                            content: result.content.clone(),
                        }),
                        _ => {}
                    }
                }
            }
            Role::Assistant => {
                for part in &message.parts {
                    match part {
                        ContentPart::Text { text } if !text.is_empty() => items.push(ChatItem {
                            role: ChatRole::Assistant,
                            content: text.clone(),
                        }),
                        ContentPart::Reasoning { text } if !text.is_empty() => {
                            items.push(ChatItem {
                                role: ChatRole::Reasoning,
                                content: text.clone(),
                            });
                        }
                        ContentPart::ToolCall(call) => items.push(ChatItem {
                            role: ChatRole::ToolCall {
                                name: call.name.clone(),
                                id: call.id.clone(),
                                args: call.arguments.to_string(),
                            },
                            content: String::new(),
                        }),
                        _ => {}
                    }
                }
            }
            Role::System => {
                let text = message.text_content();
                if !text.is_empty() {
                    items.push(ChatItem {
                        role: ChatRole::System,
                        content: text,
                    });
                }
            }
        }
    }
    items
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveModal {
    None,
    ProviderSelector {
        selected_idx: usize,
    },
    AddProvider {
        name: String,
        base_url: String,
        api_key: String,
        default_model: String,
        active_field: usize,
    },
    ModelSelector {
        models: Vec<String>,
        selected_idx: usize,
        is_loading: bool,
        filter: String,
    },
    SessionSelector {
        sessions: Vec<SessionSummary>,
        selected_idx: usize,
    },
    DiffViewer {
        diff: String,
        stat: String,
        scroll: usize,
        max_scroll: usize,
    },
    CodebaseMapViewer {
        map: String,
        scroll: usize,
        max_scroll: usize,
    },
    AuditViewer {
        content: String,
        scroll: usize,
    },
    Help,
    PrivateResult {
        tool_name: String,
        content: String,
        scroll: usize,
    },
    PermissionConfirmation {
        request: PermissionRequest,
        scroll: usize,
    },
}

pub struct App {
    pub config: AppConfig,
    pub cwd: PathBuf,
    pub session: Arc<RwLock<Session>>,
    pub store: SqliteSessionStore,
    pub tools: Arc<ToolRegistry>,
    pub context: Arc<ContextManager>,
    pub permission_manager: Arc<PermissionManager>,
    pub checkpoint_manager: Arc<CheckpointManager>,
    pub items: Vec<ChatItem>,
    pub is_running: bool,
    pub active_cancellation: Option<CancellationToken>,
    pub cancel_requested: bool,
    pub input: String,
    pub cursor_pos: usize,
    pub scroll: usize,
    pub max_scroll: usize,
    pub auto_scroll: bool,
    pub active_modal: ActiveModal,
    pub token_usage: Option<TokenUsage>,
    pub status_message: Option<(String, Instant)>,
    pub animation_frame: usize,
    pub should_quit: bool,
    pub event_tx: mpsc::UnboundedSender<AppEvent>,
    pub server_client: Option<Arc<ServerClient>>,
    pub mcp_manager: Arc<maukode_core::McpManager>,
    pub render_cache: crate::render_cache::MessageRenderCache,
}

impl App {
    pub async fn new(
        cwd: PathBuf,
        event_tx: mpsc::UnboundedSender<AppEvent>,
        session_id_to_load: Option<SessionId>,
        continue_latest: bool,
        server_client: Option<Arc<ServerClient>>,
    ) -> anyhow::Result<Self> {
        let config = AppConfig::load();
        let store = SqliteSessionStore::new(SqliteSessionStore::default_db_path())?;
        let mut tool_registry = if config.security.jail_path_traversal {
            let jail = PathJail::new(&cwd)
                .with_allowed_paths(config.security.allowed_external_paths.clone());
            create_default_registry_with_jail(&cwd, jail)
        } else {
            create_default_registry(&cwd)
        };
        config.configure_web_search_in_registry(&mut tool_registry);
        let mcp_manager = Arc::new(maukode_core::McpManager::new());
        mcp_manager
            .register_tools_into_registry(&mut tool_registry, &cwd, &config)
            .await;
        let context = Arc::new(ContextManager::new(&cwd));
        tool_registry.register(Arc::new(maukode_core::CodebaseMapTool::new(Arc::clone(
            context.repo_map(),
        ))));
        let tools = Arc::new(tool_registry);
        let permission_manager = Arc::new(
            PermissionManager::new(config.security.permission_mode)
                .with_whitelist(config.security.bash_whitelist.clone()),
        );
        let checkpoint_manager = Arc::new(CheckpointManager::new(&cwd).await?);

        // Determine session to start with
        let initial_session = if let Some(id) = session_id_to_load {
            if let Some(ref client) = server_client {
                if let Ok(detail) = client.get_session(id).await {
                    let mut s = Session::new(&detail.directory);
                    s.id = detail.id;
                    s.messages = detail.messages;
                    s.state = detail.state;
                    Some(s)
                } else {
                    store.load_session(id).await.unwrap_or(None)
                }
            } else {
                store.load_session(id).await.unwrap_or(None)
            }
            .unwrap_or_else(|| Session::new(&cwd))
        } else if continue_latest {
            store
                .get_latest_session_in_directory(&cwd)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| Session::new(&cwd))
        } else {
            Session::new(&cwd)
        };

        if let Some(ref client) = server_client {
            let client = Arc::clone(client);
            let dir = cwd.clone();
            let sid = initial_session.id;
            tokio::spawn(async move {
                if client.get_session(sid).await.is_err() {
                    let _ = client.create_session(Some(&dir), None).await;
                }
            });
        }

        let session = Arc::new(RwLock::new(initial_session));

        // Use the same conversion path for startup and later session switches.
        let mut items = {
            let guard = session
                .try_read()
                .map_err(|_| anyhow::anyhow!("session lock busy"))?;
            chat_items_from_messages(&guard.messages)
        };

        if items.is_empty() {
            items.push(ChatItem {
                role: ChatRole::System,
                content: "Welcome to MauKode! Type your prompt or use '/provider' and '/model' to configure.".to_string(),
            });
        }

        Ok(Self {
            config,
            cwd,
            session,
            store,
            tools,
            context,
            permission_manager,
            checkpoint_manager,
            items,
            is_running: false,
            active_cancellation: None,
            cancel_requested: false,
            input: String::new(),
            cursor_pos: 0,
            scroll: 0,
            max_scroll: 0,
            auto_scroll: true,
            active_modal: ActiveModal::None,
            token_usage: None,
            status_message: None,
            animation_frame: 0,
            should_quit: false,
            event_tx,
            server_client,
            mcp_manager,
            render_cache: crate::render_cache::MessageRenderCache::default(),
        })
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll = (self.scroll + amount).min(self.max_scroll);
        if self.scroll >= self.max_scroll {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
        self.scroll = self.max_scroll;
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn cancel_current_turn(&mut self) {
        if !self.is_running || self.cancel_requested {
            return;
        }
        self.cancel_requested = true;
        if let Some(token) = &self.active_cancellation {
            token.cancel();
        }
        self.set_status("Cancelling…");
    }

    /// Advance time-based visual indicators by one event-loop tick.
    pub fn tick(&mut self) {
        self.animation_frame = self.animation_frame.wrapping_add(1);
    }

    pub fn spinner(&self) -> &'static str {
        const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

        FRAMES[self.animation_frame % FRAMES.len()]
    }

    pub fn open_provider_modal(&mut self) {
        let active_id = &self.config.active_provider;
        let selected_idx = self
            .config
            .providers
            .iter()
            .position(|p| p.id == *active_id)
            .unwrap_or(0);
        self.active_modal = ActiveModal::ProviderSelector { selected_idx };
    }

    pub fn open_add_provider_modal(&mut self) {
        self.active_modal = ActiveModal::AddProvider {
            name: String::new(),
            base_url: "http://localhost:11434".to_string(),
            api_key: String::new(),
            default_model: String::new(),
            active_field: 0,
        };
    }

    pub fn open_model_modal(&mut self) {
        let provider_id = self.config.active_provider.clone();
        self.active_modal = ActiveModal::ModelSelector {
            models: Vec::new(),
            selected_idx: 0,
            is_loading: true,
            filter: String::new(),
        };

        // Fetch models asynchronously via server client if connected, or local config
        let client_opt = self.server_client.clone();
        let config_clone = self.config.clone();
        let tx = self.event_tx.clone();
        let p_id = provider_id.clone();
        tokio::spawn(async move {
            let res = if let Some(client) = client_opt {
                client
                    .list_models(Some(&p_id))
                    .await
                    .map_err(|e| e.to_string())
            } else {
                config_clone
                    .fetch_models_for_provider(&p_id)
                    .await
                    .map_err(|e| e.to_string())
            };
            let _ = tx.send(AppEvent::ModelsFetched {
                provider_id: p_id,
                models: res,
            });
        });
    }

    pub fn open_sessions_modal(&mut self) {
        let cwd = self.cwd.clone();
        let sessions = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let current_dir_sessions = self
                    .store
                    .list_sessions_by_directory(&cwd)
                    .await
                    .unwrap_or_default();
                if !current_dir_sessions.is_empty() {
                    current_dir_sessions
                } else {
                    self.store.list_all_sessions().await.unwrap_or_default()
                }
            })
        });
        self.active_modal = ActiveModal::SessionSelector {
            sessions,
            selected_idx: 0,
        };
    }

    pub fn switch_to_new_session(&mut self) {
        let new_sess = Session::new(&self.cwd);
        self.session = Arc::new(RwLock::new(new_sess));
        self.items.clear();
        self.items.push(ChatItem {
            role: ChatRole::System,
            content: format!("Started a fresh new session in '{}'.", self.cwd.display()),
        });
        self.set_status("Started new session");

        if let Some(client) = self.server_client.clone() {
            let cwd = self.cwd.clone();
            tokio::spawn(async move {
                let _ = client.create_session(Some(&cwd), None).await;
            });
        }
    }

    pub fn handle_slash_command(&mut self, line: &str) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
            "/dir" | "/pwd" => {
                self.items.push(ChatItem {
                    role: ChatRole::System,
                    content: format!("Current working directory: '{}'", self.cwd.display()),
                });
            }
            "/cd" => {
                if parts.len() < 2 {
                    self.items.push(ChatItem {
                        role: ChatRole::System,
                        content: format!("Usage: /cd <path>. Current: '{}'", self.cwd.display()),
                    });
                } else {
                    let target_str = parts[1..].join(" ");
                    let expanded = if target_str.starts_with('~') {
                        if let Some(home) = dirs::home_dir() {
                            home.join(target_str.trim_start_matches("~/").trim_start_matches('~'))
                        } else {
                            PathBuf::from(&target_str)
                        }
                    } else {
                        self.cwd.join(&target_str)
                    };

                    match std::fs::canonicalize(&expanded) {
                        Ok(canonical) => {
                            let mut tool_reg = create_default_registry(&canonical);
                            self.config.configure_web_search_in_registry(&mut tool_reg);
                            let mcp = Arc::clone(&self.mcp_manager);
                            let cfg = self.config.clone();
                            let can_clone = canonical.clone();
                            tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current().block_on(
                                    mcp.register_tools_into_registry(
                                        &mut tool_reg,
                                        &can_clone,
                                        &cfg,
                                    ),
                                )
                            });
                            let context = Arc::new(ContextManager::new(&canonical));
                            tool_reg.register(Arc::new(maukode_core::CodebaseMapTool::new(
                                Arc::clone(context.repo_map()),
                            )));
                            self.tools = Arc::new(tool_reg);
                            self.context = context;
                            if let Ok(cm) = tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(CheckpointManager::new(&canonical))
                            }) {
                                self.checkpoint_manager = Arc::new(cm);
                            }
                            if let Ok(mut sess) = self.session.try_write() {
                                sess.directory = canonical.clone();
                            }
                            self.set_status(format!("Directory: {}", canonical.display()));
                            self.items.push(ChatItem {
                                role: ChatRole::System,
                                content: format!("Changed working directory to '{}'. Tools and context reloaded.", canonical.display()),
                            });
                        }
                        Err(e) => {
                            self.set_status(format!("Cannot change dir: {e}"));
                            self.items.push(ChatItem {
                                role: ChatRole::System,
                                content: format!(
                                    "Failed to change directory to '{target_str}': {e}"
                                ),
                            });
                        }
                    }
                }
            }
            "/provider" | "/providers" => {
                if parts.len() == 1 {
                    self.open_provider_modal();
                } else if parts.len() >= 3 && parts[1] == "use" {
                    let id = parts[2];
                    if let Err(err) = self.config.set_active_provider(id) {
                        self.set_status(err.to_string());
                        self.items.push(ChatItem {
                            role: ChatRole::System,
                            content: err.to_string(),
                        });
                    } else {
                        if let Some(client) = self.server_client.clone() {
                            let id_s = id.to_string();
                            tokio::spawn(async move {
                                let _ = client.set_active_provider(&id_s).await;
                            });
                        }
                        self.set_status(format!("Active provider switched to {id}"));
                        self.items.push(ChatItem {
                            role: ChatRole::System,
                            content: format!(
                                "Switched active provider to '{}'. Active model: '{}'",
                                id, self.config.active_model
                            ),
                        });
                    }
                } else if parts.len() >= 4 && parts[1] == "add" {
                    let name = parts[2];
                    let base_url = parts[3];
                    let key = parts.get(4).map(|s| s.to_string());
                    let id = name.to_lowercase().replace(' ', "-");
                    let p = ProviderConfig {
                        id: id.clone(),
                        name: name.to_string(),
                        kind: ProviderKind::OpenAiCompatible,
                        base_url: base_url.to_string(),
                        api_key: key,
                        default_model: None,
                    };
                    self.config.add_or_update_provider(p);
                    let _ = self.config.set_active_provider(&id);
                    if let Some(client) = self.server_client.clone() {
                        let id_s = id.clone();
                        tokio::spawn(async move {
                            let _ = client.set_active_provider(&id_s).await;
                        });
                    }
                    self.set_status(format!("Added and activated provider '{name}'"));
                    self.items.push(ChatItem {
                        role: ChatRole::System,
                        content: format!(
                            "Added provider '{}' ({base_url}) and set as active.",
                            name
                        ),
                    });
                } else {
                    self.open_provider_modal();
                }
            }
            "/model" | "/models" => {
                if parts.len() == 1 {
                    self.open_model_modal();
                } else {
                    let model = parts[1..].join(" ");
                    self.config.set_active_model(&model);
                    if let Some(client) = self.server_client.clone() {
                        let m_s = model.clone();
                        tokio::spawn(async move {
                            let _ = client.set_active_model(&m_s).await;
                        });
                    }
                    self.set_status(format!("Model switched to {model}"));
                    self.items.push(ChatItem {
                        role: ChatRole::System,
                        content: format!(
                            "Switched model to '{}' for provider '{}'",
                            model, self.config.active_provider
                        ),
                    });
                }
            }
            "/privacy" | "/permission" => {
                let requested = parts.get(1).map(|value| value.to_ascii_lowercase());
                let mode = match requested.as_deref() {
                    Some("review" | "private" | "manual") => Some(PermissionMode::Review),
                    Some("strict") => Some(PermissionMode::Strict),
                    Some("ask") => Some(PermissionMode::Ask),
                    Some("trust") => Some(PermissionMode::Trust),
                    None => {
                        self.items.push(ChatItem {
                            role: ChatRole::System,
                            content: format!(
                                "Permission mode: {:?}. Usage: /permission \
                                 [review|strict|ask|trust]",
                                self.permission_manager.mode()
                            ),
                        });
                        None
                    }
                    Some(other) => {
                        self.set_status(format!("Unknown permission mode: {other}"));
                        None
                    }
                };

                if let Some(mode) = mode {
                    let previous_mode = self.permission_manager.mode();
                    self.permission_manager.set_mode(mode);
                    self.config.security.permission_mode = mode;
                    if let Err(error) = self.config.save() {
                        tracing::warn!("Failed to save permission mode: {error}");
                    }
                    if let Ok(session) = self.session.try_read() {
                        let record = maukode_core::store::sqlite::PermissionModeAuditRecord {
                            session_id: session.id,
                            turn_id: session.messages.len() as i64,
                            previous_mode,
                            new_mode: mode,
                            source: "tui_command".to_string(),
                            created_at: chrono::Utc::now(),
                        };
                        let store = self.store.clone();
                        tokio::spawn(async move {
                            if let Err(error) = store.record_permission_mode_change(&record).await {
                                tracing::warn!("Failed to persist permission mode audit: {error}");
                            }
                        });
                    }
                    if let Some(client) = self.server_client.clone() {
                        tokio::spawn(async move {
                            if let Err(error) = client.set_permission_mode(mode).await {
                                tracing::error!("Failed to update server permission mode: {error}");
                            }
                        });
                    }
                    let message = if mode == PermissionMode::Review {
                        "Review mode enabled: every tool is shown first; press p to run privately"
                            .to_string()
                    } else {
                        format!("Permission mode changed to {mode:?}")
                    };
                    self.items.push(ChatItem {
                        role: ChatRole::System,
                        content: message.clone(),
                    });
                    self.set_status(message);
                }
            }
            "/audit" => {
                let store = self.store.clone();
                let tx = self.event_tx.clone();
                let session_id = self.session.try_read().ok().map(|session| session.id);
                self.set_status("Loading audit history...");
                if let Some(session_id) = session_id {
                    tokio::spawn(async move {
                        let result = store
                            .audit_summary(session_id)
                            .await
                            .map_err(|error| error.to_string());
                        let _ = tx.send(AppEvent::AuditLoaded(result));
                    });
                } else {
                    let _ = self.event_tx.send(AppEvent::AuditLoaded(Err(
                        "Session is currently unavailable".to_string(),
                    )));
                }
            }
            "/diff" => {
                self.open_diff_modal();
            }
            "/undo" => {
                let turns = parts.get(1).and_then(|s| s.parse::<usize>().ok());
                self.undo_turn(turns);
            }
            "/map" | "/repomap" => {
                self.open_map_modal();
            }
            "/init" => {
                let force = parts.iter().any(|&p| p == "--force" || p == "-f");
                self.init_agents_file(force);
            }
            "/mcp" => {
                self.show_mcp_status();
            }
            "/compact" => {
                let focus = if parts.len() > 1 {
                    Some(parts[1..].join(" "))
                } else {
                    None
                };
                self.compact_session(focus);
            }
            "/new" => {
                self.switch_to_new_session();
            }
            "/sessions" => {
                self.open_sessions_modal();
            }
            "/clear" => {
                self.items.clear();
                self.set_status("Cleared chat display");
            }
            "/help" => {
                self.active_modal = ActiveModal::Help;
            }
            "/exit" | "/quit" => {
                self.should_quit = true;
            }
            unknown => {
                self.items.push(ChatItem {
                    role: ChatRole::System,
                    content: format!(
                        "Unknown slash command: {unknown}. Type '/help' for available commands."
                    ),
                });
            }
        }
    }

    pub fn submit_prompt(&mut self) {
        let input = std::mem::take(&mut self.input).trim().to_string();
        self.cursor_pos = 0;

        if input.is_empty() {
            return;
        }

        if input.starts_with('/') {
            self.handle_slash_command(&input);
            return;
        }

        if self.is_running {
            self.set_status("Agent is currently busy processing.");
            return;
        }

        // Add user message to display items
        self.items.push(ChatItem {
            role: ChatRole::User,
            content: input.clone(),
        });
        self.auto_scroll = true;

        if let Some(client) = self.server_client.clone() {
            self.is_running = true;
            self.cancel_requested = false;
            let cancellation = CancellationToken::new();
            self.active_cancellation = Some(cancellation.clone());
            self.set_status("Thinking (server)...");

            let session_id = {
                let guard = self.session.try_read();
                match guard {
                    Ok(s) => s.id,
                    Err(_) => SessionId::new(),
                }
            };
            let app_event_tx = self.event_tx.clone();
            let input_clone = input.clone();

            let cancel_client = Arc::clone(&client);
            let cancel_tx = app_event_tx.clone();
            tokio::spawn(async move {
                cancellation.cancelled().await;
                if let Err(error) = cancel_client.cancel_prompt(session_id).await {
                    let _ = cancel_tx.send(AppEvent::TurnFinished(Err(format!(
                        "Failed to cancel server turn: {error}"
                    ))));
                }
            });

            tokio::spawn(async move {
                // 1. Subscribe first to capture full event stream
                let mut stream = match client.subscribe_events(session_id).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = app_event_tx
                            .send(AppEvent::TurnFinished(Err(format!("Stream error: {e}"))));
                        return;
                    }
                };

                // 2. Submit prompt to initiate turn on server
                if let Err(e) = client.submit_prompt(session_id, &input_clone).await {
                    let _ = app_event_tx
                        .send(AppEvent::TurnFinished(Err(format!("Server error: {e}"))));
                    return;
                }

                // 3. Process events until completed
                let mut completed = false;
                use futures::StreamExt;
                while let Some(event_res) = stream.next().await {
                    match event_res {
                        Ok(event) => match remote_turn_result(&event) {
                            Some(Err(message)) => {
                                let _ = app_event_tx.send(AppEvent::TurnFinished(Err(message)));
                                completed = true;
                                break;
                            }
                            Some(Ok(())) => {
                                let _ = app_event_tx.send(AppEvent::LLM(event));
                                let _ = app_event_tx.send(AppEvent::TurnFinished(Ok(())));
                                return;
                            }
                            None => {
                                let _ = app_event_tx.send(AppEvent::LLM(event));
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Stream parse error: {e}");
                            let _ = app_event_tx
                                .send(AppEvent::TurnFinished(Err(format!("Stream error: {e}"))));
                            completed = true;
                            break;
                        }
                    }
                }
                if !completed {
                    let _ = app_event_tx.send(AppEvent::TurnFinished(Err(
                        "Event stream ended before the turn completed".into(),
                    )));
                }
            });
            return;
        }

        // Prepare LLM Client (standalone fallback)
        let llm_client = match self.config.create_active_llm_client() {
            Ok(c) => c,
            Err(e) => {
                self.items.push(ChatItem {
                    role: ChatRole::System,
                    content: format!("Cannot start turn: {e}. Check '/provider' settings."),
                });
                return;
            }
        };

        self.is_running = true;
        self.cancel_requested = false;
        let cancellation = CancellationToken::new();
        self.active_cancellation = Some(cancellation.clone());
        self.set_status("Thinking...");

        let session = Arc::clone(&self.session);
        let tools = Arc::clone(&self.tools);
        let context = Arc::clone(&self.context);
        let runner_config = RunnerConfig {
            model: self.config.active_model.clone(),
            max_steps: 25,
            max_tokens: Some(4096),
            temperature: None,
        };

        let runner = SessionRunner::new(session, llm_client, tools, context, runner_config)
            .with_permission_manager(Arc::clone(&self.permission_manager))
            .with_permission_store(Arc::new(self.store.clone()))
            .with_checkpoint_manager(Arc::clone(&self.checkpoint_manager));
        let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<LLMEvent>();
        let app_event_tx = self.event_tx.clone();

        // Forward LLM stream events to AppEvent
        tokio::spawn(async move {
            let run_handle = tokio::spawn(async move {
                runner
                    .run_with_cancellation(Some(input), llm_tx, Some(cancellation))
                    .await
            });

            while let Some(event) = llm_rx.recv().await {
                let _ = app_event_tx.send(AppEvent::LLM(event));
            }

            let result = match run_handle.await {
                Ok(res) => res.map_err(|e| e.to_string()),
                Err(e) => Err(format!("Task panicked: {e}")),
            };

            let _ = app_event_tx.send(AppEvent::TurnFinished(result));
        });
    }

    pub fn handle_llm_event(&mut self, event: LLMEvent) {
        match event {
            LLMEvent::PermissionRequest(req) => {
                self.active_modal = ActiveModal::PermissionConfirmation {
                    request: req,
                    scroll: 0,
                };
                self.set_status("⚠️ Action requires user confirmation");
            }
            LLMEvent::TextDelta { text } => {
                append_stream_delta(&mut self.items, ChatRole::Assistant, text);
                self.auto_scroll = true;
            }
            LLMEvent::ReasoningDelta { text } => {
                append_stream_delta(&mut self.items, ChatRole::Reasoning, text);
                self.auto_scroll = true;
            }
            LLMEvent::ToolCallStart { name, id } => {
                self.items.push(ChatItem {
                    role: ChatRole::ToolCall {
                        name,
                        id,
                        args: String::new(),
                    },
                    content: String::new(),
                });
                self.set_status("Executing tool...");
                self.auto_scroll = true;
            }
            LLMEvent::ToolCallFinish { tool_call } => {
                finish_tool_call(&mut self.items, &tool_call);
            }
            LLMEvent::ToolResult(tr) => {
                self.items.push(ChatItem {
                    role: ChatRole::ToolResult {
                        name: tr.tool_name,
                        id: tr.tool_call_id,
                        is_error: tr.is_error,
                    },
                    content: tr.content,
                });
                self.set_status("Tool executed");
                self.auto_scroll = true;
            }
            LLMEvent::PrivateToolResult(tr) => {
                self.active_modal = ActiveModal::PrivateResult {
                    tool_name: tr.tool_name,
                    content: tr.content,
                    scroll: 0,
                };
                self.set_status("Private tool result — hidden from AI");
            }
            LLMEvent::Usage(usage) => {
                self.token_usage = Some(usage);
            }
            LLMEvent::ContextWarning {
                current_tokens,
                max_tokens,
                percent,
                message,
            } => {
                self.set_status(format!("⚠️ Context {percent}% full ({current_tokens}/{max_tokens} tokens). Use /compact"));
                self.items.push(ChatItem {
                    role: ChatRole::System,
                    content: format!("⚠️ {message}"),
                });
                self.auto_scroll = true;
            }
            LLMEvent::Error { message } => {
                self.items.push(ChatItem {
                    role: ChatRole::System,
                    content: format!("Error: {message}"),
                });
            }
            _ => {}
        }
    }

    pub fn handle_turn_finished(&mut self, result: Result<(), String>) {
        self.is_running = false;
        self.active_cancellation = None;
        let was_cancelled = std::mem::take(&mut self.cancel_requested);
        match result {
            Ok(()) if was_cancelled => {
                self.set_status("Cancelled");
            }
            Ok(()) => {
                self.set_status("Idle");
            }
            Err(e) => {
                self.set_status(format!("Finished with error: {e}"));
                self.items.push(ChatItem {
                    role: ChatRole::System,
                    content: format!("Turn finished with error: {e}"),
                });
            }
        }

        // If connected to server, sync session state from server
        if let Some(client) = self.server_client.clone() {
            let session_arc = Arc::clone(&self.session);
            let session_id = {
                session_arc
                    .try_read()
                    .map(|s| s.id)
                    .unwrap_or_else(|_| SessionId::new())
            };
            tokio::spawn(async move {
                if let Ok(detail) = client.get_session(session_id).await {
                    let mut guard = session_arc.write().await;
                    guard.messages = detail.messages;
                    guard.state = detail.state;
                }
            });
            return;
        }

        // Standalone mode: persist directly to local SQLite
        let session = Arc::clone(&self.session);
        let db_path = self.store.db_path().clone();
        let provider = self.config.active_provider.clone();
        let model = self.config.active_model.clone();

        tokio::spawn(async move {
            let store = SqliteSessionStore::new(db_path);
            if let Ok(store) = store {
                let guard = session.read().await;
                let _ = store.save_session(&guard, &provider, &model).await;
            }
        });
    }

    pub fn respond_permission(&mut self, request: PermissionRequest, decision: PermissionDecision) {
        self.active_modal = ActiveModal::None;
        let action_str = match decision {
            PermissionDecision::AllowOnce => "allowed (once)",
            PermissionDecision::AllowSession => "allowed (session whitelist)",
            PermissionDecision::AllowPrivate => {
                "running privately; result will not be shared with AI"
            }
            PermissionDecision::Deny => "rejected",
        };
        self.set_status(format!("Action {action_str}"));

        if let Some(ref client) = self.server_client {
            let client = Arc::clone(client);
            let sid = match uuid::Uuid::parse_str(&request.session_id) {
                Ok(u) => SessionId(u),
                Err(_) => self
                    .session
                    .try_read()
                    .map(|s| s.id)
                    .unwrap_or_else(|_| SessionId::new()),
            };
            let req_id = request.request_id.clone();
            tokio::spawn(async move {
                if let Err(e) = client.respond_permission(sid, &req_id, decision).await {
                    tracing::error!("Failed to send permission decision to server: {e}");
                }
            });
        } else {
            let pm = Arc::clone(&self.permission_manager);
            let req_id = request.request_id.clone();
            tokio::spawn(async move {
                pm.respond(&req_id, decision).await;
            });
        }
    }

    pub fn open_diff_modal(&mut self) {
        self.set_status("Inspecting diff...");
        let tx = self.event_tx.clone();
        let session_id = {
            let guard = self.session.try_read();
            match guard {
                Ok(s) => s.id,
                Err(_) => SessionId::new(),
            }
        };

        if let Some(client) = self.server_client.clone() {
            tokio::spawn(async move {
                match client.get_diff(session_id).await {
                    Ok(diff_resp) => {
                        let _ = tx.send(AppEvent::DiffFetched(diff_resp));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::DiffFetched(maukode_protocol::DiffResponse {
                            session_id: session_id.to_string(),
                            files_changed: 0,
                            insertions: 0,
                            deletions: 0,
                            stat_summary: format!("Error fetching diff: {e}"),
                            diff_content: String::new(),
                        }));
                    }
                }
            });
        } else {
            let cm = Arc::clone(&self.checkpoint_manager);
            tokio::spawn(async move {
                let diff_resp = cm
                    .get_diff(&session_id.to_string(), None)
                    .await
                    .unwrap_or_else(|e| maukode_protocol::DiffResponse {
                        session_id: session_id.to_string(),
                        files_changed: 0,
                        insertions: 0,
                        deletions: 0,
                        stat_summary: format!("Error fetching diff: {e}"),
                        diff_content: String::new(),
                    });
                let _ = tx.send(AppEvent::DiffFetched(diff_resp));
            });
        }
    }

    pub fn undo_turn(&mut self, turns: Option<usize>) {
        let n = turns.unwrap_or(1);
        self.set_status(format!("Rolling back {n} turn(s)..."));
        let tx = self.event_tx.clone();
        let session_id = {
            let guard = self.session.try_read();
            match guard {
                Ok(s) => s.id,
                Err(_) => SessionId::new(),
            }
        };

        if let Some(client) = self.server_client.clone() {
            tokio::spawn(async move {
                match client.undo(session_id, turns).await {
                    Ok(resp) => {
                        let _ = tx.send(AppEvent::UndoCompleted(Ok(resp.message)));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::UndoCompleted(Err(e.to_string())));
                    }
                }
            });
        } else {
            let cm = Arc::clone(&self.checkpoint_manager);
            tokio::spawn(async move {
                match cm.undo(&session_id.to_string(), turns).await {
                    Ok(resp) => {
                        let _ = tx.send(AppEvent::UndoCompleted(Ok(resp.message)));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::UndoCompleted(Err(e.to_string())));
                    }
                }
            });
        }
    }

    pub fn open_map_modal(&mut self) {
        self.set_status("Analyzing codebase symbols...");
        let tx = self.event_tx.clone();
        let session_id = {
            let guard = self.session.try_read();
            match guard {
                Ok(s) => s.id,
                Err(_) => SessionId::new(),
            }
        };

        if let Some(client) = self.server_client.clone() {
            tokio::spawn(async move {
                match client
                    .get_codebase_map(session_id, None, None, Some(1000))
                    .await
                {
                    Ok(resp) => {
                        let _ = tx.send(AppEvent::CodebaseMapFetched(Ok(resp.map)));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::CodebaseMapFetched(Err(e.to_string())));
                    }
                }
            });
        } else {
            let repo_map = Arc::clone(self.context.repo_map());
            tokio::spawn(async move {
                match repo_map.generate_map(1000, None, None).await {
                    Ok(map) => {
                        let _ = tx.send(AppEvent::CodebaseMapFetched(Ok(map)));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::CodebaseMapFetched(Err(e.to_string())));
                    }
                }
            });
        }
    }

    pub fn init_agents_file(&mut self, force: bool) {
        let sid = {
            let guard = self.session.try_read();
            match guard {
                Ok(s) => s.id,
                Err(_) => SessionId::new(),
            }
        };
        let tx = self.event_tx.clone();
        self.set_status("Scanning project stack and generating standard AGENTS.md...");

        if let Some(client) = self.server_client.clone() {
            tokio::spawn(async move {
                let res = client
                    .init_project(sid, force)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(AppEvent::InitProjectCompleted(res));
            });
        } else {
            let cwd = self.cwd.clone();
            tokio::spawn(async move {
                let res = match maukode_core::init_project(&cwd, force).await {
                    Ok(r) => Ok(maukode_protocol::InitProjectResponse {
                        session_id: sid.to_string(),
                        path: r.path.display().to_string(),
                        created: r.created,
                        overwritten: r.overwritten,
                        detected_type: r.detected_type,
                        message: r.message,
                        content: r.content,
                    }),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(AppEvent::InitProjectCompleted(res));
            });
        }
    }

    pub fn show_mcp_status(&mut self) {
        let sid = {
            let guard = self.session.try_read();
            match guard {
                Ok(s) => s.id,
                Err(_) => SessionId::new(),
            }
        };
        let tx = self.event_tx.clone();
        self.set_status("Querying MCP servers...");

        if let Some(client) = self.server_client.clone() {
            tokio::spawn(async move {
                let res = client
                    .list_mcp_servers(sid)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(AppEvent::McpStatusFetched(res));
            });
        } else {
            let mcp = Arc::clone(&self.mcp_manager);
            let cwd = self.cwd.clone();
            let config = self.config.clone();
            tokio::spawn(async move {
                let servers = mcp.list_server_statuses(&cwd, &config).await;
                let _ = tx.send(AppEvent::McpStatusFetched(Ok(
                    maukode_protocol::McpListResponse { servers },
                )));
            });
        }
    }

    pub fn compact_session(&mut self, focus: Option<String>) {
        let sid = {
            let guard = self.session.try_read();
            match guard {
                Ok(s) => s.id,
                Err(_) => SessionId::new(),
            }
        };
        let tx = self.event_tx.clone();
        self.set_status("Compacting conversation history and summarizing state...");

        if let Some(client) = self.server_client.clone() {
            tokio::spawn(async move {
                let res = client
                    .compact_session(sid, focus.as_deref())
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(AppEvent::SessionCompacted(res));
            });
        } else {
            let session_arc = Arc::clone(&self.session);
            let config = self.config.clone();
            tokio::spawn(async move {
                let mut session_guard = session_arc.write().await;
                let llm_res = config.create_active_llm_client();
                let llm_client = llm_res.ok();
                let res = match maukode_core::ConversationCompactor::compact(
                    &mut session_guard,
                    llm_client.as_deref(),
                    &config.active_model,
                    focus.as_deref(),
                )
                .await
                {
                    Ok(cr) => Ok(maukode_protocol::CompactResponse {
                        session_id: sid.to_string(),
                        original_messages: cr.original_message_count,
                        compacted_messages: cr.compacted_message_count,
                        original_tokens: cr.original_tokens,
                        compacted_tokens: cr.compacted_tokens,
                        summary: cr.summary,
                    }),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(AppEvent::SessionCompacted(res));
            });
        }
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn remote_turn_finishes_on_errors_but_keeps_tool_use_streaming() {
        assert_eq!(
            remote_turn_result(&LLMEvent::Error {
                message: "provider unavailable".into(),
            }),
            Some(Err("provider unavailable".into()))
        );
        assert_eq!(
            remote_turn_result(&LLMEvent::Finish {
                reason: FinishReason::ToolUse,
            }),
            None
        );
        assert_eq!(
            remote_turn_result(&LLMEvent::Finish {
                reason: FinishReason::EndTurn,
            }),
            Some(Ok(()))
        );
    }

    #[test]
    fn deterministic_stream_burst_coalesces_chunks_within_ci_budget() {
        let mut items = Vec::new();
        let started = Instant::now();
        for _ in 0..20_000 {
            append_stream_delta(&mut items, ChatRole::Assistant, "hello world ".into());
        }
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content.len(), 240_000);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "20,000 stream chunks took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn tool_finish_updates_matching_name_and_arguments() {
        let mut items = vec![
            ChatItem {
                role: ChatRole::ToolCall {
                    name: "gre".into(),
                    id: "first".into(),
                    args: String::new(),
                },
                content: String::new(),
            },
            ChatItem {
                role: ChatRole::ToolCall {
                    name: "glob".into(),
                    id: "second".into(),
                    args: String::new(),
                },
                content: String::new(),
            },
        ];
        finish_tool_call(
            &mut items,
            &maukode_protocol::ToolCall {
                id: "first".into(),
                name: "grep".into(),
                arguments: serde_json::json!({"pattern": "hello"}),
            },
        );
        assert!(
            matches!(&items[0].role, ChatRole::ToolCall { name, args, .. }
            if name == "grep" && args.contains("hello"))
        );
        assert!(
            matches!(&items[1].role, ChatRole::ToolCall { name, args, .. }
            if name == "glob" && args.is_empty())
        );
    }
}
