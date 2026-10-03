use crate::context::ContextManager;
use crate::permission::{PermissionManager, PermissionMode};
use crate::session::Session;
use crate::store::sqlite::{PermissionAuditRecord, SqliteSessionStore};
use futures::StreamExt;
use maukode_llm::{LLMClient, LLMRequest};
use maukode_protocol::{
    ContentPart, FinishReason, LLMEvent, Message, Role, SessionState, ToolCall, ToolResult,
};
use maukode_tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info};

pub struct RunnerConfig {
    pub model: String,
    pub max_steps: usize,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            model: "claude-3-7-sonnet-20250219".to_string(),
            max_steps: 25,
            max_tokens: Some(4096),
            temperature: None,
        }
    }
}

pub struct SessionRunner {
    session: Arc<RwLock<Session>>,
    llm: Arc<dyn LLMClient>,
    tools: Arc<ToolRegistry>,
    context: Arc<ContextManager>,
    config: RunnerConfig,
    permission_manager: Option<Arc<PermissionManager>>,
    permission_store: Option<Arc<SqliteSessionStore>>,
    checkpoint_manager: Option<Arc<crate::checkpoint::CheckpointManager>>,
}

impl SessionRunner {
    pub fn new(
        session: Arc<RwLock<Session>>,
        llm: Arc<dyn LLMClient>,
        tools: Arc<ToolRegistry>,
        context: Arc<ContextManager>,
        config: RunnerConfig,
    ) -> Self {
        Self {
            session,
            llm,
            tools,
            context,
            config,
            permission_manager: None,
            permission_store: None,
            checkpoint_manager: None,
        }
    }

    pub fn with_permission_manager(mut self, pm: Arc<PermissionManager>) -> Self {
        self.permission_manager = Some(pm);
        self
    }

    pub fn with_permission_store(mut self, store: Arc<SqliteSessionStore>) -> Self {
        self.permission_store = Some(store);
        self
    }

    pub fn with_checkpoint_manager(
        mut self,
        cm: Arc<crate::checkpoint::CheckpointManager>,
    ) -> Self {
        self.checkpoint_manager = Some(cm);
        self
    }

    /// Run the multi-turn agent loop until completion, emitting events to `event_tx`.
    /// Accepts an optional `CancellationToken` for responsive cancellation.
    pub async fn run(
        &self,
        user_prompt: Option<String>,
        event_tx: mpsc::UnboundedSender<LLMEvent>,
    ) -> anyhow::Result<()> {
        self.run_with_cancellation(user_prompt, event_tx, None)
            .await
    }

    /// Run the multi-turn agent loop with cancellation support.
    pub async fn run_with_cancellation(
        &self,
        user_prompt: Option<String>,
        event_tx: mpsc::UnboundedSender<LLMEvent>,
        cancellation_token: Option<tokio_util::sync::CancellationToken>,
    ) -> anyhow::Result<()> {
        let (session_id, session_id_str, turn_idx) = {
            let mut session = self.session.write().await;
            if let Some(ref prompt) = user_prompt {
                session.add_message(Message::user(prompt.clone()));
            }
            session.state = SessionState::Running;
            (session.id, session.id.to_string(), session.messages.len())
        };

        // Automatic pre-turn shadow git snapshot before any agent tool modifications
        if let Some(ref cm) = self.checkpoint_manager {
            let desc = user_prompt.as_deref().unwrap_or("Pre-turn snapshot");
            if let Err(e) = cm.create_checkpoint(&session_id_str, turn_idx, desc).await {
                tracing::warn!("Failed to capture shadow git checkpoint: {e}");
            }
        }

        let system_prompt = self.context.render_system_context().await;
        let mut step = 0;

        loop {
            step += 1;
            if step > self.config.max_steps {
                info!(
                    "Max steps ({}) reached. Ending session turn.",
                    self.config.max_steps
                );
                let _ = event_tx.send(LLMEvent::Finish {
                    reason: FinishReason::MaxTokens,
                });
                break;
            }

            let session_guard = self.session.read().await;
            let current_messages = session_guard.messages.clone();
            drop(session_guard);

            // Context pruning: non-destructively truncate older tool outputs in LLM prompt payload
            let pruner = crate::compaction::ContextPruner::default();
            let mut pruned_messages = pruner.prune_messages(&current_messages);

            // Context window threshold monitoring
            let usage = crate::compaction::TokenEstimator::check_threshold(
                &pruned_messages,
                &system_prompt,
                &self.config.model,
            );
            if usage.exceeds_threshold {
                let pct = (usage.ratio * 100.0) as u32;
                tracing::warn!(
                    "Context window usage is high: {}/{} tokens ({}%).",
                    usage.estimated_tokens,
                    usage.context_limit,
                    pct
                );
                let _ = event_tx.send(LLMEvent::ContextWarning {
                    current_tokens: usage.estimated_tokens,
                    max_tokens: usage.context_limit,
                    percent: pct,
                    message: format!(
                        "Context window is {}% full ({}/{} tokens). Use /compact to free context space.",
                        pct, usage.estimated_tokens, usage.context_limit
                    ),
                });

                // If critical (>85%) and session has >6 messages, automatically compact older history
                if usage.is_critical && current_messages.len() > 6 {
                    tracing::info!("Context window critical ({}%). Triggering automatic conversation compaction.", pct);
                    let mut session_write = self.session.write().await;
                    if let Ok(compact_res) = crate::compaction::ConversationCompactor::compact(
                        &mut session_write,
                        Some(self.llm.as_ref()),
                        &self.config.model,
                        Some("Preserve latest active task progress and key decisions"),
                    )
                    .await
                    {
                        pruned_messages = pruner.prune_messages(&session_write.messages);
                        tracing::info!(
                            "Auto-compacted session from {} to {} messages ({} tokens saved)",
                            compact_res.original_message_count,
                            compact_res.compacted_message_count,
                            compact_res
                                .original_tokens
                                .saturating_sub(compact_res.compacted_tokens)
                        );
                    }
                }
            }

            if let Some(ref token) = cancellation_token {
                if token.is_cancelled() {
                    let mut session = self.session.write().await;
                    session.state = SessionState::Idle;
                    let _ = event_tx.send(LLMEvent::Finish {
                        reason: FinishReason::Cancelled,
                    });
                    return Ok(());
                }
            }

            let tool_defs = self.tools.definitions();
            let mut req = LLMRequest::new(&self.config.model, pruned_messages)
                .with_system_prompt(&system_prompt)
                .with_tools(tool_defs);

            req.max_tokens = self.config.max_tokens;
            req.temperature = self.config.temperature;

            debug!("Starting provider turn #{step}");
            let mut stream = self.llm.stream(req).await?;

            let mut assistant_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut provider_items = Vec::new();
            let mut _finish_reason = FinishReason::EndTurn;

            let mut is_cancelled = false;
            while let Some(event_res) = tokio::select! {
                biased;
                _ = async {
                    if let Some(ref token) = cancellation_token {
                        token.cancelled().await;
                    } else {
                        futures::future::pending::<()>().await;
                    }
                } => {
                    is_cancelled = true;
                    None
                }
                item = stream.next() => item,
            } {
                match event_res {
                    Ok(event) => {
                        match &event {
                            LLMEvent::TextDelta { text } => {
                                assistant_text.push_str(text);
                            }
                            LLMEvent::ToolCallFinish { tool_call } => {
                                tool_calls.push(tool_call.clone());
                            }
                            LLMEvent::ProviderItem { item } => {
                                provider_items.push(ContentPart::ProviderItem {
                                    value: item.clone(),
                                });
                            }
                            LLMEvent::Finish { reason } => {
                                _finish_reason = reason.clone();
                            }
                            _ => {}
                        }
                        let _ = event_tx.send(event);
                    }
                    Err(e) => {
                        error!("Error during LLM stream: {e}");
                        let _ = event_tx.send(LLMEvent::Error {
                            message: e.to_string(),
                        });
                        let mut session = self.session.write().await;
                        session.state = SessionState::Failed(e.to_string());
                        return Err(e.into());
                    }
                }
            }

            // Record assistant message
            let mut assistant_parts = Vec::new();
            assistant_parts.extend(provider_items);
            if !assistant_text.is_empty() {
                assistant_parts.push(ContentPart::Text {
                    text: assistant_text,
                });
            }
            for tc in &tool_calls {
                assistant_parts.push(ContentPart::ToolCall(tc.clone()));
            }

            let mut session = self.session.write().await;
            session.add_message(Message::assistant(assistant_parts));
            drop(session);

            // If the turn was cancelled mid-stream, stop after preserving partial output.
            if is_cancelled {
                debug!("Turn cancelled by user mid-stream.");
                let _ = event_tx.send(LLMEvent::Finish {
                    reason: FinishReason::Cancelled,
                });
                let mut session = self.session.write().await;
                session.state = SessionState::Idle;
                return Ok(());
            }

            // If there are no tool calls to execute, the turn is complete!
            if tool_calls.is_empty() {
                debug!("Turn complete with no further tool calls.");
                break;
            }

            // Execute tools
            let mut tool_results = Vec::new();
            for call in tool_calls {
                let mut keep_result_private = false;
                if let Some(ref token) = cancellation_token {
                    if token.is_cancelled() {
                        let _ = event_tx.send(LLMEvent::Finish {
                            reason: FinishReason::Cancelled,
                        });
                        let mut session = self.session.write().await;
                        session.state = SessionState::Idle;
                        return Ok(());
                    }
                }

                info!("Executing tool: {} ({})", call.name, call.id);

                let permission_key = if call.name == "bash" {
                    call.arguments
                        .get("command")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                } else {
                    serde_json::to_string(&call.arguments).ok()
                };

                if let Some(ref perm_mgr) = self.permission_manager {
                    let whitelisted = match permission_key.as_deref() {
                        Some(key) => perm_mgr.is_whitelisted(&session_id_str, key).await,
                        None => false,
                    };
                    let needs_permission = if whitelisted {
                        false
                    } else {
                        match perm_mgr.mode() {
                            PermissionMode::Trust => false,
                            PermissionMode::Strict | PermissionMode::Review => true,
                            PermissionMode::Ask => {
                                if call.name == "bash" {
                                    if let Some(cmd_val) =
                                        call.arguments.get("command").and_then(|v| v.as_str())
                                    {
                                        let assessment =
                                            maukode_tools::assess_bash_command(cmd_val);
                                        assessment.level != maukode_tools::RiskLevel::Safe
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                        }
                    };

                    if needs_permission {
                        let req_id = uuid::Uuid::new_v4().to_string();
                        let session_id_str = self.session.read().await.id.to_string();

                        let (command, risk_level, description) = if call.name == "bash" {
                            let cmd = call
                                .arguments
                                .get("command")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();
                            let assess = maukode_tools::assess_bash_command(&cmd);
                            (Some(cmd), assess.level.to_string(), assess.reason)
                        } else {
                            let details = serde_json::to_string_pretty(&call.arguments)
                                .unwrap_or_else(|_| call.arguments.to_string());
                            (
                                Some(details),
                                "warning".to_string(),
                                format!("Tool '{}' requires confirmation", call.name),
                            )
                        };

                        let perm_req = maukode_protocol::PermissionRequest {
                            request_id: req_id.clone(),
                            session_id: session_id_str.clone(),
                            tool_name: call.name.clone(),
                            command: command.clone(),
                            path: None,
                            risk_level: risk_level.clone(),
                            description,
                        };

                        let rx = perm_mgr.register_request(req_id).await;
                        let _ = event_tx.send(LLMEvent::PermissionRequest(perm_req));

                        // Wait for user's decision (timeout 5 minutes)
                        let decision =
                            match tokio::time::timeout(std::time::Duration::from_secs(300), rx)
                                .await
                            {
                                Ok(Ok(d)) => d,
                                Ok(Err(_)) => maukode_protocol::PermissionDecision::Deny,
                                Err(_) => {
                                    tracing::warn!("Permission request timed out");
                                    maukode_protocol::PermissionDecision::Deny
                                }
                            };

                        if let Some(store) = &self.permission_store {
                            let visibility =
                                if decision == maukode_protocol::PermissionDecision::AllowPrivate {
                                    "user_only"
                                } else if decision == maukode_protocol::PermissionDecision::Deny {
                                    "none"
                                } else {
                                    "model"
                                };
                            let audit = PermissionAuditRecord {
                                session_id,
                                turn_id: turn_idx as i64,
                                tool_call_id: call.id.clone(),
                                tool_name: call.name.clone(),
                                action_key: permission_key.clone().unwrap_or_default(),
                                mode: perm_mgr.mode(),
                                decision: decision.clone(),
                                risk_level: risk_level.clone(),
                                result_visibility: visibility.to_string(),
                                created_at: chrono::Utc::now(),
                            };
                            if let Err(error) = store.record_permission(&audit).await {
                                tracing::warn!(%error, "Failed to persist permission audit record");
                            }
                        }

                        match decision {
                            maukode_protocol::PermissionDecision::AllowOnce => {
                                // Proceed with execution
                            }
                            maukode_protocol::PermissionDecision::AllowSession => {
                                if let Some(ref key) = permission_key {
                                    perm_mgr
                                        .whitelist_session_command(&session_id_str, key)
                                        .await;
                                }
                            }
                            maukode_protocol::PermissionDecision::AllowPrivate => {
                                keep_result_private = true;
                            }
                            maukode_protocol::PermissionDecision::Deny => {
                                let tr = ToolResult {
                                    tool_call_id: call.id,
                                    tool_name: call.name,
                                    content: "Execution rejected by user: permission denied."
                                        .to_string(),
                                    is_error: true,
                                };
                                let _ = event_tx.send(LLMEvent::ToolResult(tr.clone()));
                                tool_results.push(ContentPart::ToolResult(tr));
                                continue;
                            }
                        }
                    }
                }

                let exec_res = if let Some(ref token) = cancellation_token {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => {
                            let _ = event_tx.send(LLMEvent::Finish {
                                reason: FinishReason::Cancelled,
                            });
                            let mut session = self.session.write().await;
                            session.state = SessionState::Idle;
                            return Ok(());
                        }
                        res = self.tools.execute(&call.name, call.arguments) => res,
                    }
                } else {
                    self.tools.execute(&call.name, call.arguments).await
                };
                let (content, is_error) = match exec_res {
                    Ok(out) => (out, false),
                    Err(err) => (format!("Error executing tool: {err}"), true),
                };

                let tr = ToolResult {
                    tool_call_id: call.id,
                    tool_name: call.name,
                    content,
                    is_error,
                };
                let _ = event_tx.send(if keep_result_private {
                    LLMEvent::PrivateToolResult(tr.clone())
                } else {
                    LLMEvent::ToolResult(tr.clone())
                });
                if keep_result_private {
                    tool_results.push(ContentPart::ToolResult(ToolResult {
                        tool_call_id: tr.tool_call_id,
                        tool_name: tr.tool_name,
                        content: "Tool executed by the user; result withheld from the model."
                            .to_string(),
                        is_error: false,
                    }));
                } else {
                    tool_results.push(ContentPart::ToolResult(tr));
                }
            }

            // Tool calls must have matching results in model history. Private results are replaced
            // by an explicit placeholder so their contents can never enter a later model request.
            let mut session = self.session.write().await;
            session.add_message(Message {
                role: Role::User,
                parts: tool_results,
            });
            drop(session);
        }

        let mut session = self.session.write().await;
        session.state = SessionState::Idle;

        Ok(())
    }
}
