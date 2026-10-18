//! Live per-session progress for dashboards: when the current turn started, how far the agent's
//! plan is, and what it did last. Kept in memory only, so it resets when the daemon restarts.

use maukode_protocol::{LLMEvent, StepStatus};

/// Longest `last_activity` line sent to clients.
const LAST_ACTIVITY_MAX: usize = 100;

/// Argument names that best describe what a tool call acts on, in priority order.
const SUMMARY_KEYS: [&str; 6] = ["command", "path", "file_path", "pattern", "query", "url"];

#[derive(Debug, Clone, Default)]
pub struct SessionActivity {
    /// RFC 3339 time the running turn began; `None` while the session is idle.
    pub turn_started_at: Option<String>,
    /// The most recent tool call, e.g. `bash cargo test`.
    pub last_activity: Option<String>,
    plan: Option<Vec<(String, StepStatus)>>,
}

impl SessionActivity {
    pub fn start_turn(&mut self, now: String) {
        self.turn_started_at = Some(now);
        self.last_activity = None;
    }

    pub fn finish_turn(&mut self) {
        self.turn_started_at = None;
    }

    /// `(completed steps, total steps)` of the active plan; `(0, 0)` when there is none.
    pub fn plan_progress(&self) -> (usize, usize) {
        match &self.plan {
            Some(steps) => (
                steps
                    .iter()
                    .filter(|(_, status)| *status == StepStatus::Completed)
                    .count(),
                steps.len(),
            ),
            None => (0, 0),
        }
    }

    /// Fold one runner event into the activity.
    pub fn observe(&mut self, event: &LLMEvent) {
        match event {
            LLMEvent::ToolCallFinish { tool_call } => {
                let detail = SUMMARY_KEYS
                    .iter()
                    .find_map(|key| tool_call.arguments.get(*key).and_then(|v| v.as_str()))
                    .unwrap_or("");
                let line = format!("{} {}", tool_call.name, detail.replace('\n', " "));
                self.last_activity = Some(truncate(line.trim()));
            }
            LLMEvent::PlanCreated { plan } => {
                self.plan = Some(
                    plan.steps
                        .iter()
                        .map(|s| (s.id.clone(), s.status))
                        .collect(),
                );
            }
            LLMEvent::PlanStepUpdated {
                step_id, status, ..
            } => {
                if let Some(steps) = &mut self.plan {
                    if let Some(step) = steps.iter_mut().find(|(id, _)| id == step_id) {
                        step.1 = *status;
                    }
                }
            }
            _ => {}
        }
    }
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= LAST_ACTIVITY_MAX {
        return s.to_string();
    }
    let cut: String = s.chars().take(LAST_ACTIVITY_MAX).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use maukode_protocol::{Plan, PlanStep, ToolCall};
    use serde_json::json;

    fn plan(statuses: &[StepStatus]) -> Plan {
        Plan {
            id: "p".into(),
            title: None,
            steps: statuses
                .iter()
                .enumerate()
                .map(|(i, status)| PlanStep {
                    id: format!("s{i}"),
                    title: format!("step {i}"),
                    status: *status,
                })
                .collect(),
        }
    }

    #[test]
    fn plan_progress_follows_step_updates() {
        let mut a = SessionActivity::default();
        assert_eq!(a.plan_progress(), (0, 0));
        a.observe(&LLMEvent::PlanCreated {
            plan: plan(&[
                StepStatus::Pending,
                StepStatus::Pending,
                StepStatus::Pending,
            ]),
        });
        assert_eq!(a.plan_progress(), (0, 3));
        a.observe(&LLMEvent::PlanStepUpdated {
            plan_id: "p".into(),
            step_id: "s1".into(),
            status: StepStatus::Completed,
        });
        assert_eq!(a.plan_progress(), (1, 3));
        // Unknown step ids are ignored.
        a.observe(&LLMEvent::PlanStepUpdated {
            plan_id: "p".into(),
            step_id: "nope".into(),
            status: StepStatus::Completed,
        });
        assert_eq!(a.plan_progress(), (1, 3));
    }

    #[test]
    fn last_activity_names_the_tool_and_its_target() {
        let mut a = SessionActivity::default();
        a.observe(&LLMEvent::ToolCallFinish {
            tool_call: ToolCall {
                id: "1".into(),
                name: "bash".into(),
                arguments: json!({ "command": "cargo test\n--all" }),
            },
        });
        assert_eq!(a.last_activity.as_deref(), Some("bash cargo test --all"));
        a.observe(&LLMEvent::ToolCallFinish {
            tool_call: ToolCall {
                id: "2".into(),
                name: "read_file".into(),
                arguments: json!({ "path": "src/main.rs" }),
            },
        });
        assert_eq!(a.last_activity.as_deref(), Some("read_file src/main.rs"));
    }

    #[test]
    fn long_activity_is_truncated() {
        let mut a = SessionActivity::default();
        a.observe(&LLMEvent::ToolCallFinish {
            tool_call: ToolCall {
                id: "1".into(),
                name: "bash".into(),
                arguments: json!({ "command": "x".repeat(500) }),
            },
        });
        assert_eq!(
            a.last_activity.unwrap().chars().count(),
            LAST_ACTIVITY_MAX + 1
        );
    }

    #[test]
    fn turn_lifecycle_sets_and_clears_start_time() {
        let mut a = SessionActivity::default();
        a.start_turn("2026-10-18T00:00:00+00:00".into());
        assert!(a.turn_started_at.is_some());
        a.finish_turn();
        assert!(a.turn_started_at.is_none());
    }
}
