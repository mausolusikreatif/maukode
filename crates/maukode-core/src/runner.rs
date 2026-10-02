use std::sync::Arc;
use futures::StreamExt;
use maukode_llm::{LLMClient, LLMRequest};
use maukode_protocol::{
    ContentPart, FinishReason, LLMEvent, Message, Role, SessionState, ToolCall, ToolResult,
};
use maukode_tools::ToolRegistry;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info};
use crate::context::ContextManager;
use crate::session::Session;

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
        }
    }

    /// Run the multi-turn agent loop until completion, emitting events to `event_tx`
    pub async fn run(
        &self,
        user_prompt: Option<String>,
        event_tx: mpsc::UnboundedSender<LLMEvent>,
    ) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        if let Some(prompt) = user_prompt {
            session.add_message(Message::user(prompt));
        }
        session.state = SessionState::Running;
        drop(session);

        let system_prompt = self.context.render_system_context().await;
        let mut step = 0;

        loop {
            step += 1;
            if step > self.config.max_steps {
                info!("Max steps ({}) reached. Ending session turn.", self.config.max_steps);
                let _ = event_tx.send(LLMEvent::Finish {
                    reason: FinishReason::MaxTokens,
                });
                break;
            }

            let session_guard = self.session.read().await;
            let current_messages = session_guard.messages.clone();
            drop(session_guard);

            let tool_defs = self.tools.definitions();
            let mut req = LLMRequest::new(&self.config.model, current_messages)
                .with_system_prompt(&system_prompt)
                .with_tools(tool_defs);

            req.max_tokens = self.config.max_tokens;
            req.temperature = self.config.temperature;

            debug!("Starting provider turn #{step}");
            let mut stream = self.llm.stream(req).await?;

            let mut assistant_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut _finish_reason = FinishReason::EndTurn;

            while let Some(event_res) = stream.next().await {
                match event_res {
                    Ok(event) => {
                        match &event {
                            LLMEvent::TextDelta { text } => {
                                assistant_text.push_str(text);
                            }
                            LLMEvent::ToolCallFinish { tool_call } => {
                                tool_calls.push(tool_call.clone());
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

            // If there are no tool calls to execute, the turn is complete!
            if tool_calls.is_empty() {
                debug!("Turn complete with no further tool calls.");
                break;
            }

            // Execute tools concurrently
            let mut tool_results = Vec::new();
            for call in tool_calls {
                info!("Executing tool: {} ({})", call.name, call.id);
                let exec_res = self.tools.execute(&call.name, call.arguments).await;
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
                let _ = event_tx.send(LLMEvent::ToolResult(tr.clone()));
                tool_results.push(ContentPart::ToolResult(tr));
            }

            // Append tool results as user role message for LLM continuation
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
