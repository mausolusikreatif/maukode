use crate::context::ContextManager;
use crate::permission::{PermissionManager, PermissionMode};
use crate::session::Session;
use crate::store::sqlite::{PermissionAuditRecord, SqliteSessionStore};
use futures::StreamExt;
use maukode_llm::{LLMClient, LLMRequest};
use maukode_protocol::{
    ContentPart, FinishReason, LLMEvent, Message, Role, SessionState, StopReason, ToolCall,
    ToolResult,
};
use maukode_tools::{ToolError, ToolRegistry};
use std::collections::HashMap;
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
            // Resolve the model from the active provider configuration instead of pinning a
            // provider-specific literal. A stale literal breaks gateways that do not expose that
            // exact model name, so callers that do not pick a model explicitly inherit the
            // configured default.
            model: crate::AppConfig::load().active_model,
            max_steps: 2000,
            max_tokens: Some(8192),
            temperature: None,
        }
    }
}

/// How many times a response cut off by the output limit is resumed automatically.
const MAX_AUTO_CONTINUATIONS: usize = 2;

/// Total provider attempts per step (1 initial + retries). Transient transport
/// failures (gateway EOF/reset, 429, 5xx) are retried inside the step so one
/// blip doesn't fail the whole turn.
const MAX_STREAM_ATTEMPTS: usize = 4;

/// Nudge used when resuming a text-only turn that died mid-stream. The partial
/// text was already forwarded to the UI, so the model continues from it
/// instead of regenerating (which would duplicate the visible output).
const STREAM_RESUME_PROMPT: &str = "Your previous response was cut off by a connection error. \
    Continue exactly where you left off, without repeating what you already wrote.";

/// Tools without side effects. When every call of a step is one of these they run concurrently.
const READ_ONLY_TOOLS: &[&str] = &[
    "fetch_web_content",
    "get_codebase_map",
    "glob",
    "grep",
    "memory_browse",
    "memory_catalog",
    "memory_get",
    "memory_search",
    "read_file",
    "web_search",
];

/// Every tool call stored in history must have a matching result, or the next provider request
/// is rejected. Build "cancelled" results for the calls (`(id, name)`) that never got one.
fn cancelled_results(calls: &[(String, String)], answered: &[ContentPart]) -> Vec<ContentPart> {
    calls
        .iter()
        .filter(|(id, _)| {
            !answered
                .iter()
                .any(|p| matches!(p, ContentPart::ToolResult(r) if &r.tool_call_id == id))
        })
        .map(|(id, name)| {
            ContentPart::ToolResult(ToolResult {
                tool_call_id: id.clone(),
                tool_name: name.clone(),
                content: "Cancelled by user before this tool ran.".to_string(),
                is_error: true,
            })
        })
        .collect()
}

fn stopped(reason: StopReason, message: Option<String>) -> LLMEvent {
    LLMEvent::TurnStopped { reason, message }
}

/// Backoff between transient-failure retries. Returns true when cancellation
/// was requested while waiting.
async fn sleep_or_cancelled(
    cancellation_token: &Option<tokio_util::sync::CancellationToken>,
    stream_attempt: usize,
) -> bool {
    // Short backoff: exhaustion must surface within seconds so the UI (and
    // the e2e suite) sees the failure promptly instead of hanging the turn.
    let backoff = std::time::Duration::from_millis((200u64 << stream_attempt.min(3)).min(1500));
    if let Some(ref token) = cancellation_token {
        tokio::select! {
            biased;
            _ = token.cancelled() => true,
            _ = tokio::time::sleep(backoff) => false,
        }
    } else {
        tokio::time::sleep(backoff).await;
        false
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
    memory: Option<crate::context::MemoryBinding>,
    reflection_config: Option<crate::reflection::ReflectionConfig>,
    consolidation_config: Option<crate::consolidation::ConsolidationConfig>,
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
            memory: None,
            reflection_config: None,
            consolidation_config: None,
        }
    }

    /// Attach the knowledge-store binding. Enables end-of-turn extraction when the binding has
    /// `auto_extract` set; the tools themselves are registered separately by the caller.
    pub fn with_memory(mut self, memory: crate::context::MemoryBinding) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Enable post-turn reflection. When set, the agent will evaluate its performance
    /// after each completed turn and extract lessons / propose skills.
    pub fn with_reflection_config(mut self, config: crate::reflection::ReflectionConfig) -> Self {
        self.reflection_config = Some(config);
        self
    }

    /// Enable post-turn memory consolidation. When set, the agent will detect and
    /// reconcile near-duplicate memories after each completed turn.
    pub fn with_consolidation_config(
        mut self,
        config: crate::consolidation::ConsolidationConfig,
    ) -> Self {
        self.consolidation_config = Some(config);
        self
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
    /// Run one agent turn. Every call ends the event stream with exactly one
    /// [`LLMEvent::TurnStopped`] (after an [`LLMEvent::Error`] on failure) so clients never have
    /// to guess whether the agent is done.
    pub async fn run_with_cancellation(
        &self,
        user_prompt: Option<String>,
        event_tx: mpsc::UnboundedSender<LLMEvent>,
        cancellation_token: Option<tokio_util::sync::CancellationToken>,
    ) -> anyhow::Result<()> {
        match self
            .run_turn(user_prompt, event_tx.clone(), cancellation_token)
            .await
        {
            Ok((reason, message)) => {
                let _ = event_tx.send(stopped(reason, message));
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                error!("Agent turn failed: {message}");
                self.session.write().await.state = SessionState::Failed(message.clone());
                let _ = event_tx.send(LLMEvent::Error {
                    message: message.clone(),
                });
                let _ = event_tx.send(stopped(StopReason::Error, Some(message)));
                Err(e)
            }
        }
    }

    async fn run_turn(
        &self,
        user_prompt: Option<String>,
        event_tx: mpsc::UnboundedSender<LLMEvent>,
        cancellation_token: Option<tokio_util::sync::CancellationToken>,
    ) -> anyhow::Result<(StopReason, Option<String>)> {
        let (session_id, session_id_str, mut turn_idx) = {
            let mut session = self.session.write().await;
            if let Some(ref prompt) = user_prompt {
                session.add_message(Message::user(prompt.clone()));
            }
            session.state = SessionState::Running;
            (session.id, session.id.to_string(), session.messages.len())
        };

        if let Some(store) = &self.permission_store {
            if let Ok(stable_turn_id) = store.start_turn(session_id, turn_idx as i64).await {
                turn_idx = stable_turn_id as usize;
            }
        }

        // Automatic pre-turn shadow git snapshot before any agent tool modifications
        if let Some(ref cm) = self.checkpoint_manager {
            let desc = user_prompt.as_deref().unwrap_or("Pre-turn snapshot");
            if let Err(e) = cm.create_checkpoint(&session_id_str, turn_idx, desc).await {
                tracing::warn!("Failed to capture shadow git checkpoint: {e}");
            }
        }

        // The stable part is sent first so providers can cache it; the date and recalled
        // memories follow it.
        let system_prompt = self.context.render_system_context().await;
        let system_suffix = self
            .context
            .render_turn_context(user_prompt.as_deref())
            .await;
        let estimated_system = format!("{system_prompt}\n{system_suffix}");
        let mut step = 0;
        let mut continuations = 0usize;
        let mut stop: (StopReason, Option<String>) = (StopReason::Completed, None);

        loop {
            step += 1;
            if step > self.config.max_steps {
                info!(
                    "Max steps ({}) reached. Ending session turn.",
                    self.config.max_steps
                );
                stop = (
                    StopReason::MaxSteps,
                    Some(format!(
                        "Reached the step limit ({}) before the task finished. Send a message \
                         (e.g. \"continue\") to resume, or raise the limit.",
                        self.config.max_steps
                    )),
                );
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
                &estimated_system,
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
                        if let Some(store) = &self.permission_store {
                            let audit = crate::store::sqlite::CompactionAuditRecord {
                                session_id,
                                source_turn_start: 0,
                                source_turn_end: turn_idx as i64,
                                original_message_count: compact_res.original_message_count,
                                compacted_message_count: compact_res.compacted_message_count,
                                original_tokens: compact_res.original_tokens,
                                compacted_tokens: compact_res.compacted_tokens,
                                trigger: "automatic_threshold".to_string(),
                                summary: compact_res.summary.clone(),
                                created_at: chrono::Utc::now(),
                            };
                            if let Err(error) = store.record_compaction(&audit).await {
                                tracing::warn!(%error, "Failed to persist compaction audit record");
                            }
                        }
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
                    return Ok((StopReason::Cancelled, None));
                }
            }

            let tool_defs = self.tools.definitions();

            debug!("Starting provider turn #{step}");

            let mut assistant_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut provider_items = Vec::new();
            let mut finish_reason = FinishReason::EndTurn;

            // Retry state for transient stream failures within this step.
            let mut stream_attempt: usize = 0;
            let mut resume_suffix: Vec<Message> = Vec::new();
            let mut saw_tool_activity = false;
            let mut is_cancelled = false;

            'attempts: loop {
                let mut req = LLMRequest::new(&self.config.model, pruned_messages.clone())
                    .with_system_prompt(&system_prompt)
                    .with_system_suffix(&system_suffix)
                    .with_tools(tool_defs.clone());
                req.max_tokens = self.config.max_tokens;
                req.temperature = self.config.temperature;
                if !resume_suffix.is_empty() {
                    req.messages.extend(resume_suffix.clone());
                }

                let mut stream = match self.llm.stream(req).await {
                    Ok(s) => s,
                    Err(e) if e.is_transient() && stream_attempt + 1 < MAX_STREAM_ATTEMPTS => {
                        stream_attempt += 1;
                        debug!("Provider stream setup failed transiently ({e}); retrying...");
                        if sleep_or_cancelled(&cancellation_token, stream_attempt).await {
                            is_cancelled = true;
                            break 'attempts;
                        }
                        continue 'attempts;
                    }
                    // The caller reports the failure (Error + TurnStopped) and marks the session.
                    Err(e) => return Err(e.into()),
                };

                let mut attempt_broken = false;
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
                                    saw_tool_activity = true;
                                    tool_calls.push(tool_call.clone());
                                }
                                LLMEvent::ToolCallStart { .. } | LLMEvent::ToolCallDelta { .. } => {
                                    saw_tool_activity = true;
                                }
                                LLMEvent::ProviderItem { item } => {
                                    provider_items.push(ContentPart::ProviderItem {
                                        value: item.clone(),
                                    });
                                }
                                LLMEvent::Finish { reason } => {
                                    finish_reason = reason.clone();
                                }
                                _ => {}
                            }
                            let _ = event_tx.send(event);
                        }
                        Err(e) if e.is_transient() && stream_attempt + 1 < MAX_STREAM_ATTEMPTS => {
                            stream_attempt += 1;
                            debug!("Provider stream failed transiently ({e}); retrying...");
                            attempt_broken = true;
                            break;
                        }
                        // The caller reports the failure (Error + TurnStopped) and marks the session.
                        Err(e) => return Err(e.into()),
                    }
                }
                if !attempt_broken {
                    break 'attempts;
                }

                if !saw_tool_activity && !assistant_text.is_empty() {
                    // Resume a text-only turn from the partial text instead of
                    // regenerating it (which would duplicate visible output).
                    resume_suffix = vec![
                        Message::assistant(vec![ContentPart::Text {
                            text: assistant_text.clone(),
                        }]),
                        Message::user(STREAM_RESUME_PROMPT),
                    ];
                } else {
                    // Nothing resumable (setup failure, or tool calls in flight
                    // whose results don't exist yet): restart the step cleanly.
                    assistant_text.clear();
                    tool_calls.clear();
                    provider_items.clear();
                    resume_suffix.clear();
                    saw_tool_activity = false;
                }
                if sleep_or_cancelled(&cancellation_token, stream_attempt).await {
                    is_cancelled = true;
                    break 'attempts;
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

            // An empty assistant message is invalid for several providers and would poison the
            // next request, so never store one.
            let empty_response = assistant_parts.is_empty();
            if !empty_response {
                let mut session = self.session.write().await;
                session.add_message(Message::assistant(assistant_parts));
            }

            // If the turn was cancelled mid-stream, stop after preserving partial output.
            if is_cancelled {
                debug!("Turn cancelled by user mid-stream.");
                if !tool_calls.is_empty() {
                    let meta: Vec<(String, String)> = tool_calls
                        .iter()
                        .map(|c| (c.id.clone(), c.name.clone()))
                        .collect();
                    self.session.write().await.add_message(Message {
                        role: Role::User,
                        parts: cancelled_results(&meta, &[]),
                    });
                }
                let _ = event_tx.send(LLMEvent::Finish {
                    reason: FinishReason::Cancelled,
                });
                let mut session = self.session.write().await;
                session.state = SessionState::Idle;
                return Ok((StopReason::Cancelled, None));
            }

            if empty_response {
                stop = (
                    StopReason::Error,
                    Some("The model returned an empty response. Try again or switch model.".into()),
                );
                break;
            }

            if finish_reason != FinishReason::MaxTokens {
                continuations = 0;
            }

            // No tool calls: the model is done, unless the output limit cut it off mid-answer.
            if tool_calls.is_empty() {
                if finish_reason == FinishReason::MaxTokens {
                    if continuations < MAX_AUTO_CONTINUATIONS {
                        continuations += 1;
                        info!("Response hit the output limit; auto-continuing ({continuations})");
                        self.session.write().await.add_message(Message::user(
                            "Your previous response was cut off by the output limit. Continue \
                             exactly where you left off.",
                        ));
                        continue;
                    }
                    stop = (
                        StopReason::MaxTokens,
                        Some(
                            "The response was cut off by the output limit (max_tokens) and the \
                             task is probably unfinished. Send \"continue\" to resume, or raise \
                             max_tokens."
                                .into(),
                        ),
                    );
                }
                debug!("Turn complete with no further tool calls.");
                break;
            }

            // Execute tools
            let mut tool_results = Vec::new();
            let call_meta: Vec<(String, String)> = tool_calls
                .iter()
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect();
            let mut cancelled = false;
            let mut prefetched = self
                .run_read_only_batch(&tool_calls, &cancellation_token)
                .await;
            for call in tool_calls {
                let mut keep_result_private = false;
                if cancellation_token
                    .as_ref()
                    .is_some_and(|t| t.is_cancelled())
                {
                    cancelled = true;
                    break;
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
                        Some(key) => {
                            perm_mgr
                                .is_whitelisted(&session_id_str, &call.name, key)
                                .await
                        }
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

                        let rx = perm_mgr.register_request(req_id.clone()).await;
                        let _ = event_tx.send(LLMEvent::PermissionRequest(perm_req));

                        // Wait for the user's decision (5 minute timeout); Stop must not have to
                        // wait for it.
                        let decision = tokio::select! {
                            biased;
                            _ = async {
                                match &cancellation_token {
                                    Some(token) => token.cancelled().await,
                                    None => futures::future::pending::<()>().await,
                                }
                            } => {
                                perm_mgr.forget_request(&req_id).await;
                                cancelled = true;
                                break;
                            }
                            waited = tokio::time::timeout(std::time::Duration::from_secs(300), rx) => {
                                match waited {
                                    Ok(Ok(d)) => d,
                                    Ok(Err(_)) => maukode_protocol::PermissionDecision::Deny,
                                    Err(_) => {
                                        tracing::warn!("Permission request timed out");
                                        perm_mgr.forget_request(&req_id).await;
                                        maukode_protocol::PermissionDecision::Deny
                                    }
                                }
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
                                decision,
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
                                        .whitelist_session_command(&session_id_str, &call.name, key)
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

                let exec_res = if let Some(res) = prefetched.remove(&call.id) {
                    res
                } else if let Some(ref token) = cancellation_token {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => {
                            cancelled = true;
                            break;
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

            if cancelled {
                let extra = cancelled_results(&call_meta, &tool_results);
                tool_results.extend(extra);
                let mut session = self.session.write().await;
                session.add_message(Message {
                    role: Role::User,
                    parts: tool_results,
                });
                session.state = SessionState::Idle;
                drop(session);
                let _ = event_tx.send(LLMEvent::Finish {
                    reason: FinishReason::Cancelled,
                });
                return Ok((StopReason::Cancelled, None));
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
        let final_messages = session.messages.clone();
        drop(session);

        // End-of-turn extraction: fold any auto-captured candidate memories into the store. Runs
        // only on a clean completion and only when the binding opted in.
        if stop.0 == StopReason::Completed {
            self.extract_memories(&final_messages).await;

            // Post-turn reflection: the agent evaluates its own performance and extracts
            // lessons / proposes skills. Runs only on clean completion with reflection enabled.
            self.reflect_on_turn(&final_messages).await;

            // Post-turn memory consolidation: detect and reconcile near-duplicate
            // memories. Runs only on clean completion with consolidation enabled.
            self.consolidate_memories().await;
        }

        Ok(stop)
    }

    /// Run the calls of a step concurrently when all of them only read, so none can miss a write
    /// that comes before it, and none of them needs the user's confirmation. Returns the results
    /// by call id; an empty map means the caller runs the calls one by one as usual.
    async fn run_read_only_batch(
        &self,
        calls: &[ToolCall],
        cancellation_token: &Option<tokio_util::sync::CancellationToken>,
    ) -> HashMap<String, Result<String, ToolError>> {
        let confirms_every_call = self
            .permission_manager
            .as_ref()
            .is_some_and(|pm| matches!(pm.mode(), PermissionMode::Strict | PermissionMode::Review));
        let all_read_only = calls
            .iter()
            .all(|call| READ_ONLY_TOOLS.contains(&call.name.as_str()));
        if calls.len() < 2 || confirms_every_call || !all_read_only {
            return HashMap::new();
        }

        let batch = futures::future::join_all(calls.iter().map(|call| async move {
            let result = self.tools.execute(&call.name, call.arguments.clone()).await;
            (call.id.clone(), result)
        }));
        match cancellation_token {
            Some(token) => tokio::select! {
                biased;
                _ = token.cancelled() => HashMap::new(),
                results = batch => results.into_iter().collect(),
            },
            None => batch.await.into_iter().collect(),
        }
    }

    /// Run the reflection engine over the completed turn, if configured. Never fails
    /// the turn — all errors are logged internally.
    async fn reflect_on_turn(&self, messages: &[maukode_protocol::Message]) {
        let Some(reflection_config) = &self.reflection_config else {
            return;
        };
        if !reflection_config.enabled {
            return;
        }
        let Some(memory) = &self.memory else {
            return;
        };

        // Count tool calls in the conversation to gate reflection.
        let tool_call_count = messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .filter(|p| matches!(p, maukode_protocol::ContentPart::ToolCall(..)))
            .count();

        let engine = crate::reflection::ReflectionEngine::new(
            &self.llm,
            &self.config.model,
            memory,
            self.context.cwd(),
            reflection_config,
        );
        engine.reflect(messages, tool_call_count).await;
    }

    /// Run the memory consolidation engine, if configured. Detects near-duplicate
    /// memories and reconciles them via LLM-guided merge/supersede decisions.
    /// Never fails the turn — all errors are logged internally.
    async fn consolidate_memories(&self) {
        let Some(config) = &self.consolidation_config else {
            return;
        };
        if !config.enabled {
            return;
        }
        let Some(binding) = &self.memory else {
            return;
        };
        let engine = crate::consolidation::ConsolidationEngine::new(
            &self.llm,
            &self.config.model,
            &binding.context,
            config.clone(),
        );
        engine.run().await;
    }

    /// Persist auto-extracted candidate memories from a finished turn, logging (never failing the
    /// turn) on error. Explicit `memory_write` tool calls are the primary path; this is a safety
    /// net for durable notes a turn leaves behind automatically.
    async fn extract_memories(&self, messages: &[maukode_protocol::Message]) {
        let Some(binding) = &self.memory else {
            return;
        };
        if !binding.auto_extract {
            return;
        }
        let candidates = maukode_memory::extract::candidates_from_messages(messages);
        if candidates.is_empty() {
            return;
        }
        match maukode_memory::persist_candidates(
            binding.store(),
            binding.scope(),
            &candidates,
            binding.context.author.as_deref(),
        )
        .await
        {
            Ok(report) => {
                debug!(
                    "Memory extraction: {} proposed, {} written, {} updated, {} skipped",
                    report.proposed, report.written, report.updated, report.skipped
                );
            }
            Err(e) => tracing::warn!("Memory extraction failed: {e}"),
        }
    }
}

#[cfg(test)]
mod turn_tests {
    use super::*;
    use maukode_llm::{LLMError, LLMStream};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    type Step = Result<Vec<Result<LLMEvent, LLMError>>, LLMError>;

    /// Replays scripted provider responses, one per request.
    struct ScriptedLlm(Mutex<VecDeque<Step>>);

    #[async_trait::async_trait]
    impl LLMClient for ScriptedLlm {
        async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
            let step = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("script exhausted");
            step.map(|events| Box::pin(futures::stream::iter(events)) as LLMStream)
        }
    }

    fn text(t: &str, reason: FinishReason) -> Step {
        Ok(vec![
            Ok(LLMEvent::TextDelta { text: t.into() }),
            Ok(LLMEvent::Finish { reason }),
        ])
    }

    async fn run(
        script: Vec<Step>,
        max_steps: usize,
    ) -> (
        Vec<LLMEvent>,
        anyhow::Result<()>,
        Vec<Message>,
        SessionState,
    ) {
        let dir = std::env::temp_dir();
        let session = Arc::new(RwLock::new(Session::new(&dir)));
        let runner = SessionRunner::new(
            Arc::clone(&session),
            Arc::new(ScriptedLlm(Mutex::new(script.into()))),
            Arc::new(ToolRegistry::new()),
            Arc::new(ContextManager::new(&dir)),
            RunnerConfig {
                max_steps,
                ..RunnerConfig::default()
            },
        );
        let (tx, mut rx) = mpsc::unbounded_channel();
        let result = runner
            .run_with_cancellation(Some("go".into()), tx, None)
            .await;
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        let guard = session.read().await;
        (events, result, guard.messages.clone(), guard.state.clone())
    }

    fn stops(events: &[LLMEvent]) -> Vec<(StopReason, Option<String>)> {
        events
            .iter()
            .filter_map(|e| match e {
                LLMEvent::TurnStopped { reason, message } => Some((*reason, message.clone())),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn normal_completion_emits_one_completed() {
        let (events, result, _, _) = run(vec![text("done", FinishReason::EndTurn)], 5).await;
        assert!(result.is_ok());
        assert_eq!(stops(&events), vec![(StopReason::Completed, None)]);
        assert!(matches!(events.last(), Some(LLMEvent::TurnStopped { .. })));
    }

    #[tokio::test]
    async fn truncated_response_is_auto_continued() {
        let (events, _, messages, _) = run(
            vec![
                text("part one ", FinishReason::MaxTokens),
                text("part two", FinishReason::EndTurn),
            ],
            5,
        )
        .await;
        assert_eq!(stops(&events), vec![(StopReason::Completed, None)]);
        // user prompt, assistant part 1, synthetic continue, assistant part 2
        assert_eq!(messages.len(), 4);
        assert!(messages[2].text_content().contains("cut off"));
    }

    #[tokio::test]
    async fn persistent_truncation_reports_max_tokens() {
        let script = (0..=MAX_AUTO_CONTINUATIONS)
            .map(|_| text("x", FinishReason::MaxTokens))
            .collect();
        let (events, _, _, _) = run(script, 10).await;
        let s = stops(&events);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, StopReason::MaxTokens);
        assert!(s[0].1.as_deref().unwrap().contains("continue"));
    }

    #[tokio::test]
    async fn step_limit_is_reported_as_max_steps_not_max_tokens() {
        let call = |id: &str| -> Step {
            Ok(vec![
                Ok(LLMEvent::ToolCallFinish {
                    tool_call: ToolCall {
                        id: id.into(),
                        name: "missing_tool".into(),
                        arguments: serde_json::json!({}),
                    },
                }),
                Ok(LLMEvent::Finish {
                    reason: FinishReason::ToolUse,
                }),
            ])
        };
        let (events, result, _, _) = run(vec![call("1"), call("2")], 2).await;
        assert!(result.is_ok());
        let s = stops(&events);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, StopReason::MaxSteps);
        // No misleading provider-level MaxTokens finish.
        assert!(!events.iter().any(|e| matches!(
            e,
            LLMEvent::Finish {
                reason: FinishReason::MaxTokens
            }
        )));
    }

    #[tokio::test]
    async fn provider_failures_always_close_with_error() {
        // Failure before the stream opens: previously produced no event at all.
        let (events, result, _, state) = run(vec![Err(LLMError::Stream("boom".into()))], 5).await;
        assert!(result.is_err());
        assert!(events.iter().any(|e| matches!(e, LLMEvent::Error { .. })));
        let s = stops(&events);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, StopReason::Error);
        assert!(matches!(state, SessionState::Failed(_)));
    }

    #[tokio::test]
    async fn empty_response_is_an_error_and_not_stored() {
        let (events, _, messages, _) = run(
            vec![Ok(vec![Ok(LLMEvent::Finish {
                reason: FinishReason::EndTurn,
            })])],
            5,
        )
        .await;
        let s = stops(&events);
        assert_eq!(s[0].0, StopReason::Error);
        assert!(s[0].1.as_deref().unwrap().contains("empty"));
        // Only the user prompt: no empty assistant message that would poison the next request.
        assert_eq!(messages.len(), 1);
    }

    /// A gateway that reports `Finish(EndTurn)` alongside tool calls must not end the turn: the
    /// runner keeps going until a response has no tool calls.
    #[tokio::test]
    async fn tool_calls_continue_even_if_provider_reports_stop() {
        let call: Step = Ok(vec![
            Ok(LLMEvent::ToolCallFinish {
                tool_call: ToolCall {
                    id: "1".into(),
                    name: "missing_tool".into(),
                    arguments: serde_json::json!({}),
                },
            }),
            Ok(LLMEvent::Finish {
                reason: FinishReason::EndTurn,
            }),
        ]);
        let (events, result, messages, _) =
            run(vec![call, text("all done", FinishReason::EndTurn)], 5).await;
        assert!(result.is_ok());
        // user, assistant(tool call), user(tool result), assistant(text)
        assert_eq!(messages.len(), 4);
        assert_eq!(stops(&events), vec![(StopReason::Completed, None)]);
        let finishes = events
            .iter()
            .filter(|e| matches!(e, LLMEvent::Finish { .. }))
            .count();
        assert_eq!(finishes, 2, "the early Finish arrives before the turn ends");
        let first_finish = events
            .iter()
            .position(|e| matches!(e, LLMEvent::Finish { .. }))
            .unwrap();
        let stop = events
            .iter()
            .position(|e| matches!(e, LLMEvent::TurnStopped { .. }))
            .unwrap();
        assert!(first_finish < stop);
    }

    #[tokio::test]
    async fn transient_setup_failure_is_retried() {
        let (events, result, _, _) = run(
            vec![
                Err(LLMError::Stream("EOF while streaming".into())),
                text("done", FinishReason::EndTurn),
            ],
            5,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(stops(&events), vec![(StopReason::Completed, None)]);
    }

    #[tokio::test]
    async fn mid_stream_eof_resumes_text_without_duplication() {
        let broken: Step = Ok(vec![
            Ok(LLMEvent::TextDelta {
                text: "part ".into(),
            }),
            Err(LLMError::Stream("connection reset by peer".into())),
        ]);
        let (events, result, messages, _) =
            run(vec![broken, text("two", FinishReason::EndTurn)], 5).await;
        assert!(result.is_ok());
        assert_eq!(stops(&events), vec![(StopReason::Completed, None)]);
        // The stored assistant message joins text from both sides of the resume.
        let assistant = messages
            .iter()
            .find(|m| m.role == Role::Assistant)
            .expect("assistant message stored");
        assert_eq!(assistant.text_content(), "part two");
        // Already-forwarded text is continued, never regenerated.
        let deltas: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                LLMEvent::TextDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["part ".to_string(), "two".to_string()]);
    }

    #[tokio::test]
    async fn persistent_transient_failure_closes_with_error() {
        let script: Vec<Step> = (0..MAX_STREAM_ATTEMPTS)
            .map(|_| Err(LLMError::Stream("EOF".into())))
            .collect();
        let (events, result, _, state) = run(script, 5).await;
        assert!(result.is_err());
        assert!(events.iter().any(|e| matches!(e, LLMEvent::Error { .. })));
        assert_eq!(stops(&events).len(), 1);
        assert_eq!(stops(&events)[0].0, StopReason::Error);
        assert!(matches!(state, SessionState::Failed(_)));
    }

    /// A read tool that only returns once two calls are running at the same time.
    struct RendezvousTool(Arc<tokio::sync::Barrier>);

    #[async_trait::async_trait]
    impl maukode_tools::Tool for RendezvousTool {
        fn name(&self) -> &str {
            "read_file"
        }
        fn description(&self) -> &str {
            "test"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<String, ToolError> {
            self.0.wait().await;
            Ok("read".into())
        }
    }

    #[tokio::test]
    async fn read_only_calls_of_one_step_run_concurrently() {
        let call = |id: &str| {
            Ok(LLMEvent::ToolCallFinish {
                tool_call: ToolCall {
                    id: id.into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({}),
                },
            })
        };
        let script: Vec<Step> = vec![
            Ok(vec![
                call("1"),
                call("2"),
                Ok(LLMEvent::Finish {
                    reason: FinishReason::ToolUse,
                }),
            ]),
            text("done", FinishReason::EndTurn),
        ];
        let dir = std::env::temp_dir();
        let session = Arc::new(RwLock::new(Session::new(&dir)));
        let mut tools = ToolRegistry::new();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        tools.register(Arc::new(RendezvousTool(barrier)));
        let runner = SessionRunner::new(
            Arc::clone(&session),
            Arc::new(ScriptedLlm(Mutex::new(script.into()))),
            Arc::new(tools),
            Arc::new(ContextManager::new(&dir)),
            RunnerConfig {
                max_steps: 5,
                ..RunnerConfig::default()
            },
        );
        let (tx, _rx) = mpsc::unbounded_channel();
        // Run one after the other, the first call would wait for the second forever.
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            runner.run_with_cancellation(Some("go".into()), tx, None),
        )
        .await
        .expect("the read-only calls ran one at a time")
        .unwrap();

        let guard = session.read().await;
        let results: Vec<_> = guard
            .messages
            .iter()
            .flat_map(|m| &m.parts)
            .filter_map(|p| match p {
                ContentPart::ToolResult(r) => Some(r.content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(results, ["read", "read"]);
    }
}
