//! Machine-readable output for `maukode run` (`--output json` / `stream-json`).

use maukode_protocol::{LLMEvent, SessionUsage, StopReason, TokenUsage};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable progress (the default).
    #[default]
    Text,
    /// One JSON object when the turn ends.
    Json,
    /// Every event as one JSON line, then the result object as the last line.
    StreamJson,
}

impl OutputFormat {
    pub fn is_machine(self) -> bool {
        self != OutputFormat::Text
    }
}

#[derive(Debug, Default)]
struct ToolCallReport {
    id: String,
    name: String,
    arguments: Value,
    is_error: Option<bool>,
}

/// What a headless turn did, collected from its events.
#[derive(Debug, Default)]
pub struct RunReport {
    pub session_id: String,
    pub stop_reason: Option<StopReason>,
    pub message: Option<String>,
    text: String,
    /// The next text delta starts a new provider response.
    new_step: bool,
    tool_calls: Vec<ToolCallReport>,
    pub denied_permissions: Vec<String>,
    errors: Vec<String>,
    pub usage: SessionUsage,
    /// The latest session totals the daemon reported; the caller turns them into this turn's
    /// usage (see [`usage_since`]).
    pub server_totals: Option<SessionUsage>,
}

impl RunReport {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            ..Self::default()
        }
    }

    /// Fold one event in. `cost` prices a response (local runs; the daemon prices its own).
    pub fn observe(&mut self, event: &LLMEvent, cost: impl Fn(&TokenUsage) -> Option<f64>) {
        match event {
            LLMEvent::TextDelta { text } => {
                if self.new_step && !self.text.is_empty() {
                    self.text.push_str("\n\n");
                }
                self.new_step = false;
                self.text.push_str(text);
            }
            LLMEvent::Finish { .. } => self.new_step = true,
            LLMEvent::ToolCallFinish { tool_call } => self.tool_calls.push(ToolCallReport {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
                is_error: None,
            }),
            LLMEvent::ToolResult(result) | LLMEvent::PrivateToolResult(result) => {
                if let Some(call) = self
                    .tool_calls
                    .iter_mut()
                    .find(|c| c.id == result.tool_call_id)
                {
                    call.is_error = Some(result.is_error);
                }
            }
            LLMEvent::Usage(usage) => self.usage.add(usage, cost(usage)),
            LLMEvent::SessionUsage(totals) => self.server_totals = Some(totals.clone()),
            LLMEvent::Error { message } => self.errors.push(message.clone()),
            LLMEvent::TurnStopped { reason, message } => {
                self.stop_reason = Some(*reason);
                self.message = message.clone();
            }
            _ => {}
        }
    }

    /// The turn failed (an error stopped it, or it never reported how it ended).
    pub fn failed(&self) -> bool {
        self.stop_reason
            .is_none_or(|reason| reason == StopReason::Error)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "type": "result",
            "session_id": self.session_id,
            "stop_reason": self.stop_reason,
            "message": self.message,
            "is_error": self.failed(),
            "result": self.text,
            "tool_calls": self.tool_calls.iter().map(|call| json!({
                "id": call.id,
                "name": call.name,
                "arguments": call.arguments,
                "is_error": call.is_error,
            })).collect::<Vec<_>>(),
            "denied_permissions": self.denied_permissions,
            "errors": self.errors,
            "usage": self.usage,
        })
    }
}

/// `after` minus `before`, for a turn's share of a session's totals.
pub fn usage_since(after: &SessionUsage, before: &SessionUsage) -> SessionUsage {
    SessionUsage {
        input_tokens: after.input_tokens.saturating_sub(before.input_tokens),
        output_tokens: after.output_tokens.saturating_sub(before.output_tokens),
        cache_read_tokens: after
            .cache_read_tokens
            .saturating_sub(before.cache_read_tokens),
        cache_write_tokens: after
            .cache_write_tokens
            .saturating_sub(before.cache_write_tokens),
        cost_microusd: after.cost_microusd.saturating_sub(before.cost_microusd),
        unpriced_responses: after
            .unpriced_responses
            .saturating_sub(before.unpriced_responses),
    }
}

/// One `stream-json` line for an event.
pub fn event_line(event: &LLMEvent) -> String {
    serde_json::to_string(event).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use maukode_protocol::{FinishReason, ToolCall, ToolResult};

    #[test]
    fn report_collects_text_tools_usage_and_the_stop() {
        let mut report = RunReport::new("s1");
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 10,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };
        let events = [
            LLMEvent::TextDelta {
                text: "Look".into(),
            },
            LLMEvent::ToolCallFinish {
                tool_call: ToolCall {
                    id: "c1".into(),
                    name: "bash".into(),
                    arguments: json!({"command": "ls"}),
                },
            },
            LLMEvent::Usage(usage.clone()),
            LLMEvent::Finish {
                reason: FinishReason::ToolUse,
            },
            LLMEvent::ToolResult(ToolResult {
                tool_call_id: "c1".into(),
                tool_name: "bash".into(),
                content: "a".into(),
                is_error: false,
            }),
            LLMEvent::TextDelta {
                text: "Done".into(),
            },
            LLMEvent::Usage(usage),
            LLMEvent::TurnStopped {
                reason: StopReason::Completed,
                message: None,
            },
        ];
        for event in &events {
            report.observe(event, |_| Some(0.5));
        }
        let out = report.to_json();
        assert_eq!(out["result"], "Look\n\nDone");
        assert_eq!(out["stop_reason"], "completed");
        assert_eq!(out["is_error"], false);
        assert_eq!(out["tool_calls"][0]["name"], "bash");
        assert_eq!(out["tool_calls"][0]["is_error"], false);
        assert_eq!(out["usage"]["input_tokens"], 200);
        assert_eq!(out["usage"]["cost_microusd"], 1_000_000);
    }

    #[test]
    fn a_turn_without_a_clean_stop_is_an_error() {
        let mut report = RunReport::new("s");
        assert!(report.failed());
        report.observe(
            &LLMEvent::TurnStopped {
                reason: StopReason::MaxSteps,
                message: Some("limit".into()),
            },
            |_| None,
        );
        assert!(!report.failed());
        report.observe(
            &LLMEvent::TurnStopped {
                reason: StopReason::Error,
                message: None,
            },
            |_| None,
        );
        assert!(report.failed());
    }

    #[test]
    fn server_totals_are_kept_for_the_caller() {
        let mut report = RunReport::new("s");
        let totals = SessionUsage {
            input_tokens: 7,
            ..SessionUsage::default()
        };
        report.observe(&LLMEvent::SessionUsage(totals.clone()), |_| None);
        assert_eq!(report.server_totals, Some(totals));
        let before = SessionUsage {
            input_tokens: 10,
            ..SessionUsage::default()
        };
        let after = SessionUsage {
            input_tokens: 25,
            unpriced_responses: 1,
            ..SessionUsage::default()
        };
        assert_eq!(usage_since(&after, &before).input_tokens, 15);
        assert_eq!(usage_since(&after, &before).unpriced_responses, 1);
    }
}
