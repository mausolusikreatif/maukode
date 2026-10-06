use crate::context::ContextManager;
use crate::runner::{RunnerConfig, SessionRunner};
use crate::session::Session;
use async_trait::async_trait;
use maukode_llm::LLMClient;
use maukode_protocol::{
    LLMEvent, ModelTier, Role, SessionId, SubagentProfile, SubagentStatus, SubagentToolMode,
};
use maukode_tools::{PathJail, Tool, ToolError};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[derive(Deserialize)]
struct SubagentArgs {
    profile: String,
    goal: String,
    context_hints: Option<Vec<String>>,
}

pub struct SubagentTool {
    parent_session_id: SessionId,
    directory: PathBuf,
    jail: PathJail,
    llm: Arc<dyn LLMClient>,
    event_tx: Arc<RwLock<Option<mpsc::UnboundedSender<LLMEvent>>>>,
    profiles: Arc<RwLock<HashMap<String, SubagentProfile>>>,
}

impl SubagentTool {
    pub fn new(
        parent_session_id: SessionId,
        directory: impl Into<PathBuf>,
        jail: PathJail,
        llm: Arc<dyn LLMClient>,
        event_tx: Option<mpsc::UnboundedSender<LLMEvent>>,
    ) -> Self {
        let mut initial_profiles = HashMap::new();
        let researcher = SubagentProfile::codebase_researcher();
        let auditor = SubagentProfile::security_auditor();
        initial_profiles.insert(researcher.name.clone(), researcher);
        initial_profiles.insert(auditor.name.clone(), auditor);

        Self {
            parent_session_id,
            directory: directory.into(),
            jail,
            llm,
            event_tx: Arc::new(RwLock::new(event_tx)),
            profiles: Arc::new(RwLock::new(initial_profiles)),
        }
    }

    pub async fn register_profile(&self, profile: SubagentProfile) {
        let mut profiles = self.profiles.write().await;
        profiles.insert(profile.name.clone(), profile);
    }

    pub async fn set_event_sender(&self, tx: Option<mpsc::UnboundedSender<LLMEvent>>) {
        let mut guard = self.event_tx.write().await;
        *guard = tx;
    }
}

#[async_trait]
impl Tool for SubagentTool {
    fn name(&self) -> &str {
        "invoke_subagent"
    }

    fn description(&self) -> &str {
        "Delegate an isolated research, audit, or exploration subtask to a specialized child subagent with a dedicated context window."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "profile": {
                    "type": "string",
                    "description": "The subagent profile to deploy (e.g., 'codebase-researcher' or 'security-auditor')"
                },
                "goal": {
                    "type": "string",
                    "description": "Clear, detailed task goal and expected findings"
                },
                "context_hints": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of relevant files, symbols, or clues to prime the subagent"
                }
            },
            "required": ["profile", "goal"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, ToolError> {
        let args: SubagentArgs = serde_json::from_value(arguments).map_err(|e| {
            ToolError::InvalidParameters(format!("Invalid subagent parameters: {e}"))
        })?;

        let profile = {
            let guard = self.profiles.read().await;
            guard
                .get(&args.profile)
                .cloned()
                .unwrap_or_else(SubagentProfile::codebase_researcher)
        };

        let child_subagent_id = format!("subagent-{}", uuid::Uuid::new_v4());

        let event_sender = self.event_tx.read().await.clone();
        if let Some(ref tx) = event_sender {
            let _ = tx.send(LLMEvent::SubagentSpawned {
                subagent_id: child_subagent_id.clone(),
                parent_session_id: self.parent_session_id,
                profile_name: profile.name.clone(),
                goal: args.goal.clone(),
            });
        }

        let mut child_tools = match profile.tool_mode {
            SubagentToolMode::ReadOnly => maukode_tools::create_readonly_registry_with_jail(
                &self.directory,
                self.jail.clone(),
            ),
            SubagentToolMode::WorkspaceMutating => {
                maukode_tools::create_default_registry_with_jail(&self.directory, self.jail.clone())
            }
        };

        let child_context = Arc::new(ContextManager::new(&self.directory));
        child_tools.register(Arc::new(crate::repo_map::CodebaseMapTool::new(Arc::clone(
            child_context.repo_map(),
        ))));

        let child_session = Arc::new(RwLock::new(Session::new(&self.directory)));

        let mut child_prompt = format!(
            "Role & Specialist Mission:\n{}\n\nGoal:\n{}\n",
            profile.role_prompt, args.goal
        );
        if let Some(hints) = args.context_hints {
            if !hints.is_empty() {
                child_prompt.push_str("\nContext Hints:\n");
                for hint in hints {
                    child_prompt.push_str(&format!("- {hint}\n"));
                }
            }
        }

        let child_runner_config = RunnerConfig {
            model: match profile.model_tier {
                ModelTier::Inherit => "claude-3-7-sonnet-20250219".to_string(),
                ModelTier::Fast => "claude-3-5-haiku-20241022".to_string(),
                ModelTier::Strong => "claude-3-7-sonnet-20250219".to_string(),
            },
            max_steps: profile.max_turns,
            max_tokens: profile.max_tokens.map(|t| t as u32).or(Some(8192)),
            temperature: None,
        };

        let child_runner = SessionRunner::new(
            Arc::clone(&child_session),
            Arc::clone(&self.llm),
            Arc::new(child_tools),
            child_context,
            child_runner_config,
        );

        let (child_tx, mut child_rx) = mpsc::unbounded_channel::<LLMEvent>();
        let event_sender_task = event_sender.clone();
        let sub_id_task = child_subagent_id.clone();

        let progress_tracker = tokio::spawn(async move {
            let mut step = 0;
            let mut tokens = 0;
            while let Some(event) = child_rx.recv().await {
                match &event {
                    LLMEvent::ToolCallStart { name, .. } => {
                        step += 1;
                        if let Some(ref tx) = event_sender_task {
                            let _ = tx.send(LLMEvent::SubagentProgress {
                                subagent_id: sub_id_task.clone(),
                                step,
                                action: format!("Running tool: {name}"),
                                tokens_used: tokens,
                            });
                        }
                    }
                    LLMEvent::Usage(usage) => {
                        tokens = (usage.input_tokens + usage.output_tokens) as usize;
                    }
                    _ => {}
                }
            }
            tokens
        });

        let run_result = child_runner.run(Some(child_prompt), child_tx).await;
        let tokens_used = progress_tracker.await.unwrap_or(0);

        let (status, summary) = match run_result {
            Ok(()) => {
                let guard = child_session.read().await;
                let last_content = guard
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == Role::Assistant)
                    .map(|m| m.text_content())
                    .unwrap_or_else(|| "Subagent completed the research objective.".to_string());
                (SubagentStatus::Completed, last_content)
            }
            Err(e) => {
                let msg = e.to_string();
                (
                    SubagentStatus::Failed,
                    format!("Subagent encountered an error: {msg}"),
                )
            }
        };

        if let Some(ref tx) = event_sender {
            let _ = tx.send(LLMEvent::SubagentFinished {
                subagent_id: child_subagent_id,
                status,
                summary: summary.clone(),
                tokens_used,
            });
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maukode_llm::{LLMError, LLMRequest, LLMStream};
    use maukode_protocol::FinishReason;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    type Step = Result<Vec<Result<LLMEvent, LLMError>>, LLMError>;

    struct ScriptedLlm(Mutex<VecDeque<Step>>);

    #[async_trait]
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

    #[tokio::test]
    async fn test_subagent_invocation_and_events() {
        let script = vec![Ok(vec![
            Ok(LLMEvent::TextDelta {
                text: "Research findings: Found 3 relevant symbols.".to_string(),
            }),
            Ok(LLMEvent::Finish {
                reason: FinishReason::EndTurn,
            }),
        ])];

        let llm = Arc::new(ScriptedLlm(Mutex::new(script.into())));
        let dir = std::env::temp_dir();
        let jail = PathJail::new(&dir);
        let parent_id = SessionId::new();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let tool = SubagentTool::new(parent_id, dir, jail, llm, Some(tx));
        let args = json!({
            "profile": "codebase-researcher",
            "goal": "Find authentication race condition",
            "context_hints": ["src/auth.rs"]
        });

        let res = tool.execute(args).await.unwrap();
        assert!(res.contains("Research findings: Found 3 relevant symbols."));

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        assert!(events
            .iter()
            .any(|e| matches!(e, LLMEvent::SubagentSpawned { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, LLMEvent::SubagentFinished { .. })));
    }
}
