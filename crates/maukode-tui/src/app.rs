use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use maukode_core::{
    AppConfig, ContextManager, ProviderConfig, ProviderKind, RunnerConfig, Session, SessionRunner,
    SessionSummary, SqliteSessionStore,
};
use maukode_protocol::{ContentPart, LLMEvent, Role, SessionId, TokenUsage};
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
    pub auto_scroll: bool,
    pub active_modal: ActiveModal,
    pub token_usage: Option<TokenUsage>,
    pub status_message: Option<(String, Instant)>,
    pub should_quit: bool,
    pub event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl App {
    pub fn new(
        cwd: PathBuf,
        event_tx: mpsc::UnboundedSender<AppEvent>,
        session_id_to_load: Option<SessionId>,
        continue_latest: bool,
    ) -> anyhow::Result<Self> {
        let config = AppConfig::load();
        let store = SqliteSessionStore::new(SqliteSessionStore::default_db_path())?;
        let tools = Arc::new(create_default_registry(&cwd));
        let context = Arc::new(ContextManager::new(&cwd));

        // Determine session to start with
        let initial_session = if let Some(id) = session_id_to_load {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    store.load_session(id).await.unwrap_or(None)
                })
            }).unwrap_or_else(Session::new)
        } else if continue_latest {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    store.get_latest_session().await.unwrap_or(None)
                })
            }).unwrap_or_else(Session::new)
        } else {
            Session::new()
        };

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
            auto_scroll: true,
            active_modal: ActiveModal::None,
            token_usage: None,
            status_message: None,
            should_quit: false,
            event_tx,
        })
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
        let sessions = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.store.list_sessions().await.unwrap_or_default()
            })
        });
        self.active_modal = ActiveModal::SessionSelector {
            sessions,
            selected_idx: 0,
        };
    }

    pub fn switch_to_new_session(&mut self) {
        let new_sess = Session::new();
        self.session = Arc::new(RwLock::new(new_sess));
        self.items.clear();
        self.items.push(ChatItem {
            role: ChatRole::System,
            content: "Started a fresh new session.".to_string(),
        });
        self.set_status("Started new session");
    }

    pub fn handle_slash_command(&mut self, line: &str) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
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

        // Prepare LLM Client
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

        // Persist session to SQLite!
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
