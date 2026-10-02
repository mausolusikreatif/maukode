use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use maukode_core::{
    AppConfig, ContextManager, ProviderConfig, ProviderKind, RunnerConfig, Session, SessionRunner,
    SessionSummary, SqliteSessionStore,
};
use maukode_protocol::{ContentPart, LLMEvent, Role, SessionId, TokenUsage};
use maukode_server::ServerClient;
use maukode_tools::{create_default_registry, ToolRegistry};
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
    Help,
}

pub struct App {
    pub config: AppConfig,
    pub cwd: PathBuf,
    pub session: Arc<RwLock<Session>>,
    pub store: SqliteSessionStore,
    pub tools: Arc<ToolRegistry>,
    pub context: Arc<ContextManager>,
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
    pub fn new(
        cwd: PathBuf,
        event_tx: mpsc::UnboundedSender<AppEvent>,
        session_id_to_load: Option<SessionId>,
        continue_latest: bool,
        server_client: Option<Arc<ServerClient>>,
    ) -> anyhow::Result<Self> {
        let config = AppConfig::load();
        let store = SqliteSessionStore::new(SqliteSessionStore::default_db_path())?;
        let tools = Arc::new(create_default_registry(&cwd));
        let context = Arc::new(ContextManager::new(&cwd));

        // Determine session to start with
        let initial_session = if let Some(id) = session_id_to_load {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    if let Some(ref client) = server_client {
                        if let Ok(detail) = client.get_session(id).await {
                            let mut s = Session::new(&detail.directory);
                            s.id = detail.id;
                            s.messages = detail.messages;
                            s.state = detail.state;
                            return Some(s);
                        }
                    }
                    store.load_session(id).await.unwrap_or(None)
                })
            }).unwrap_or_else(|| Session::new(&cwd))
        } else if continue_latest {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    store.get_latest_session_in_directory(&cwd).await.unwrap_or(None)
                })
            }).unwrap_or_else(|| Session::new(&cwd))
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

        // Fetch models asynchronously
        let config_clone = self.config.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let res = config_clone
                .fetch_models_for_provider(&provider_id)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::ModelsFetched {
                provider_id,
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
                            self.cwd = canonical.clone();
                            self.tools = Arc::new(create_default_registry(&canonical));
                            self.context = Arc::new(ContextManager::new(&canonical));
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
                    self.set_status(format!("Model switched to {model}"));
                    self.items.push(ChatItem {
                        role: ChatRole::System,
                        content: format!("Switched model to '{}' for provider '{}'", model, self.config.active_provider),
                    });
                }
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

        let runner = SessionRunner::new(session, llm_client, tools, context, runner_config);
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
}
