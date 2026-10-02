use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use maukode_core::{
    AppConfig, CheckpointManager, ContextManager, PermissionManager, ProviderConfig, ProviderKind,
    RunnerConfig, Session, SessionRunner, SessionSummary, SqliteSessionStore,
};
use maukode_protocol::{
    ContentPart, LLMEvent, PermissionDecision, PermissionRequest, Role, SessionId, TokenUsage,
};
use maukode_server::ServerClient;
use maukode_tools::{create_default_registry, create_default_registry_with_jail, PathJail, ToolRegistry};
use tokio::sync::{mpsc, RwLock};
use crate::event::AppEvent;

#[derive(Debug, Clone)]
pub enum ChatRole {
    User,
    Assistant,
    System,
    ToolCall { name: String, id: String, args: String },
    ToolResult { name: String, id: String, is_error: bool },
    Reasoning,
}

#[derive(Debug, Clone)]
pub struct ChatItem {
    pub role: ChatRole,
    pub content: String,
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
    Help,
    PermissionConfirmation {
        request: PermissionRequest,
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
    pub input: String,
    pub cursor_pos: usize,
    pub scroll: usize,
    pub max_scroll: usize,
    pub auto_scroll: bool,
    pub active_modal: ActiveModal,
    pub token_usage: Option<TokenUsage>,
    pub status_message: Option<(String, Instant)>,
    pub should_quit: bool,
    pub event_tx: mpsc::UnboundedSender<AppEvent>,
    pub server_client: Option<Arc<ServerClient>>,
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
        let context = Arc::new(ContextManager::new(&cwd));
        tool_registry.register(Arc::new(maukode_core::CodebaseMapTool::new(Arc::clone(context.repo_map()))));
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

        // Convert existing messages to chat items
        let mut items = Vec::new();
        {
            let guard = session.try_read().unwrap();
            for msg in &guard.messages {
                match msg.role {
                    Role::User => {
                        let text = msg.text_content();
                        if !text.is_empty() {
                            items.push(ChatItem {
                                role: ChatRole::User,
                                content: text,
                            });
                        }
                        for part in &msg.parts {
                            if let ContentPart::ToolResult(tr) = part {
                                items.push(ChatItem {
                                    role: ChatRole::ToolResult {
                                        name: tr.tool_name.clone(),
                                        id: tr.tool_call_id.clone(),
                                        is_error: tr.is_error,
                                    },
                                    content: tr.content.clone(),
                                });
                            }
                        }
                    }
                    Role::Assistant => {
                        for part in &msg.parts {
                            match part {
                                ContentPart::Text { text } => {
                                    items.push(ChatItem {
                                        role: ChatRole::Assistant,
                                        content: text.clone(),
                                    });
                                }
                                ContentPart::Reasoning { text } => {
                                    items.push(ChatItem {
                                        role: ChatRole::Reasoning,
                                        content: text.clone(),
                                    });
                                }
                                ContentPart::ToolCall(tc) => {
                                    items.push(ChatItem {
                                        role: ChatRole::ToolCall {
                                            name: tc.name.clone(),
                                            id: tc.id.clone(),
                                            args: tc.arguments.to_string(),
                                        },
                                        content: String::new(),
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    Role::System => {}
                }
            }
        }

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
            input: String::new(),
            cursor_pos: 0,
            scroll: 0,
            max_scroll: 0,
            auto_scroll: true,
            active_modal: ActiveModal::None,
            token_usage: None,
            status_message: None,
            should_quit: false,
            event_tx,
            server_client,
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
                let current_dir_sessions = self.store.list_sessions_by_directory(&cwd).await.unwrap_or_default();
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
                            let context = Arc::new(ContextManager::new(&canonical));
                            tool_reg.register(Arc::new(maukode_core::CodebaseMapTool::new(Arc::clone(context.repo_map()))));
                            self.tools = Arc::new(tool_reg);
                            self.context = context;
                            if let Ok(cm) = tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current().block_on(CheckpointManager::new(&canonical))
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
                                content: format!("Failed to change directory to '{target_str}': {e}"),
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
                    if let Ok(()) = self.config.set_active_provider(id) {
                        if let Some(client) = self.server_client.clone() {
                            let id_s = id.to_string();
                            tokio::spawn(async move {
                                let _ = client.set_active_provider(&id_s).await;
                            });
                        }
                        self.set_status(format!("Active provider switched to {id}"));
                        self.items.push(ChatItem {
                            role: ChatRole::System,
                            content: format!("Switched active provider to '{}'. Active model: '{}'", id, self.config.active_model),
                        });
                    } else {
                        self.set_status(format!("Provider '{id}' not found"));
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
                        content: format!("Added provider '{}' ({base_url}) and set as active.", name),
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
                        content: format!("Switched model to '{}' for provider '{}'", model, self.config.active_provider),
                    });
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
                    content: format!("Unknown slash command: {unknown}. Type '/help' for available commands."),
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

            tokio::spawn(async move {
                // 1. Subscribe first to capture full event stream
                let mut stream = match client.subscribe_events(session_id).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = app_event_tx.send(AppEvent::TurnFinished(Err(format!("Stream error: {e}"))));
                        return;
                    }
                };

                // 2. Submit prompt to initiate turn on server
                if let Err(e) = client.submit_prompt(session_id, &input_clone).await {
                    let _ = app_event_tx.send(AppEvent::TurnFinished(Err(format!("Server error: {e}"))));
                    return;
                }

                // 3. Process events until completed
                use futures::StreamExt;
                while let Some(event_res) = stream.next().await {
                    match event_res {
                        Ok(event) => {
                            let is_finish = matches!(event, LLMEvent::Finish { .. });
                            let _ = app_event_tx.send(AppEvent::LLM(event));
                            if is_finish {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Stream parse error: {e}");
                            break;
                        }
                    }
                }
                let _ = app_event_tx.send(AppEvent::TurnFinished(Ok(())));
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
            .with_checkpoint_manager(Arc::clone(&self.checkpoint_manager));
        let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<LLMEvent>();
        let app_event_tx = self.event_tx.clone();

        // Forward LLM stream events to AppEvent
        tokio::spawn(async move {
            let run_handle = tokio::spawn(async move {
                runner.run(Some(input), llm_tx).await
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
                self.active_modal = ActiveModal::PermissionConfirmation { request: req };
                self.set_status("⚠️ Action requires user confirmation");
            }
            LLMEvent::TextDelta { text } => {
                if let Some(last) = self.items.last_mut() {
                    if matches!(last.role, ChatRole::Assistant) {
                        last.content.push_str(&text);
                        return;
                    }
                }
                self.items.push(ChatItem {
                    role: ChatRole::Assistant,
                    content: text,
                });
                self.auto_scroll = true;
            }
            LLMEvent::ReasoningDelta { text } => {
                if let Some(last) = self.items.last_mut() {
                    if matches!(last.role, ChatRole::Reasoning) {
                        last.content.push_str(&text);
                        return;
                    }
                }
                self.items.push(ChatItem {
                    role: ChatRole::Reasoning,
                    content: text,
                });
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
                if let Some(last) = self.items.last_mut() {
                    if let ChatRole::ToolCall { ref mut args, .. } = last.role {
                        *args = tool_call.arguments.to_string();
                    }
                }
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
            LLMEvent::Usage(usage) => {
                self.token_usage = Some(usage);
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
        match result {
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
                session_arc.try_read().map(|s| s.id).unwrap_or_else(|_| SessionId::new())
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

    pub fn respond_permission(
        &mut self,
        request: PermissionRequest,
        decision: PermissionDecision,
    ) {
        self.active_modal = ActiveModal::None;
        let action_str = match decision {
            PermissionDecision::AllowOnce => "allowed (once)",
            PermissionDecision::AllowSession => "allowed (session whitelist)",
            PermissionDecision::Deny => "rejected",
        };
        self.set_status(format!("Action {action_str}"));

        if let Some(ref client) = self.server_client {
            let client = Arc::clone(client);
            let sid = match uuid::Uuid::parse_str(&request.session_id) {
                Ok(u) => SessionId(u),
                Err(_) => self.session.try_read().map(|s| s.id).unwrap_or_else(|_| SessionId::new()),
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
                let diff_resp = cm.get_diff(&session_id.to_string(), None).await.unwrap_or_else(|e| {
                    maukode_protocol::DiffResponse {
                        session_id: session_id.to_string(),
                        files_changed: 0,
                        insertions: 0,
                        deletions: 0,
                        stat_summary: format!("Error fetching diff: {e}"),
                        diff_content: String::new(),
                    }
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
                match client.get_codebase_map(session_id, None, None, Some(1000)).await {
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
}
