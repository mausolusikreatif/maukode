use crate::message::{ToolCall, ToolResult};
use crate::session::SessionId;
use crate::subagent::SubagentStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Cancelled,
    Error,
}

/// Why an agent turn ended. Exactly one `LLMEvent::TurnStopped` closes every turn; per-step
/// `Finish` events are provider-level and must not be used to decide the turn is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model finished its answer.
    Completed,
    /// The output limit cut the response off (auto-continue was exhausted).
    MaxTokens,
    /// The step limit was reached with work possibly unfinished.
    MaxSteps,
    Cancelled,
    /// Provider/stream failure or an empty model response; see the message.
    Error,
}

impl StopReason {
    /// True when the user probably needs to send "continue" to finish the task.
    pub fn needs_continue(self) -> bool {
        matches!(self, Self::MaxTokens | Self::MaxSteps)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TokenUsage {
    /// The whole prompt, including the cached part.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub input_tokens: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub output_tokens: u64,
    /// The part of `input_tokens` read from the provider's prompt cache.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub cache_read_tokens: Option<u64>,
    /// The part of `input_tokens` written to the provider's prompt cache.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub cache_write_tokens: Option<u64>,
}

/// Tokens a session has used so far, over all of its responses (subagents included), and what
/// they cost where the model's price is known.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SessionUsage {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub input_tokens: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub output_tokens: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub cache_read_tokens: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub cache_write_tokens: u64,
    /// Cost in millionths of a US dollar, of the responses whose model has a known price.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub cost_microusd: u64,
    /// Responses whose model had no known price; their tokens are counted, their cost is not.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub unpriced_responses: u64,
    /// The input size of the session's latest request: what the model's context held then. Not a
    /// running total, unlike the fields above.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    #[serde(default)]
    pub context_tokens: u64,
}

impl SessionUsage {
    /// Add one response; `cost_usd` is `None` when its model has no known price.
    pub fn add(&mut self, usage: &TokenUsage, cost_usd: Option<f64>) {
        self.input_tokens += usage.input_tokens;
        self.context_tokens = usage.input_tokens;
        self.output_tokens += usage.output_tokens;
        self.cache_read_tokens += usage.cache_read_tokens.unwrap_or(0);
        self.cache_write_tokens += usage.cache_write_tokens.unwrap_or(0);
        match cost_usd {
            Some(cost) => self.cost_microusd += (cost * 1_000_000.0).round() as u64,
            None => self.unpriced_responses += 1,
        }
    }

    /// Add the totals of `other` (persisting a delta).
    pub fn merge(&mut self, other: &SessionUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.cost_microusd += other.cost_microusd;
        self.unpriced_responses += other.unpriced_responses;
        if other.context_tokens > 0 {
            self.context_tokens = other.context_tokens;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowSession,
    /// Execute the tool locally, show the result to the user, and do not send the result to the
    /// model. The current agent turn stops after all pending tool calls have completed.
    AllowPrivate,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub status: StepStatus,
}

impl PlanStep {
    pub fn new(id: impl Into<String>, title: impl Into<String>, status: StepStatus) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Plan {
    pub id: String,
    pub title: Option<String>,
    pub steps: Vec<PlanStep>,
}

impl Plan {
    pub fn new(id: impl Into<String>, steps: Vec<PlanStep>) -> Self {
        Self {
            id: id.into(),
            title: None,
            steps,
        }
    }

    pub fn with_title(
        id: impl Into<String>,
        title: impl Into<String>,
        steps: Vec<PlanStep>,
    ) -> Self {
        Self {
            id: id.into(),
            title: Some(title.into()),
            steps,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PermissionRequest {
    pub request_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub command: Option<String>,
    pub path: Option<String>,
    pub risk_level: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ProviderItem {
        item: serde_json::Value,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        delta: String,
    },
    ToolCallFinish {
        tool_call: ToolCall,
    },
    ToolResult(ToolResult),
    /// A real tool result shown only in the user's private-result popup.
    PrivateToolResult(ToolResult),
    PermissionRequest(PermissionRequest),
    Usage(TokenUsage),
    /// The session's totals after a `Usage` event (published by the daemon).
    SessionUsage(SessionUsage),
    ContextWarning {
        current_tokens: usize,
        max_tokens: usize,
        percent: u32,
        message: String,
    },
    /// A structured plan was created for the agent's task.
    PlanCreated {
        plan: Plan,
    },
    /// A single step of an active plan transitioned status.
    PlanStepUpdated {
        plan_id: String,
        step_id: String,
        status: StepStatus,
    },
    /// A specialized child subagent was spawned by the parent agent.
    SubagentSpawned {
        subagent_id: String,
        parent_session_id: SessionId,
        profile_name: String,
        goal: String,
    },
    /// Fine-grained progress update from an executing subagent.
    SubagentProgress {
        subagent_id: String,
        step: usize,
        action: String,
        tokens_used: usize,
    },
    /// A child subagent completed its execution.
    SubagentFinished {
        subagent_id: String,
        status: SubagentStatus,
        summary: String,
        tokens_used: usize,
    },
    /// A single provider response ended (one step of a possibly multi-step turn).
    Finish {
        reason: FinishReason,
    },
    Error {
        message: String,
    },
    /// The whole agent turn is over. Always the last event of a turn.
    TurnStopped {
        reason: StopReason,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub turn_index: usize,
    pub description: String,
    pub tree_hash: String,
    pub commit_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiffResponse {
    pub session_id: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub stat_summary: String,
    pub diff_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UndoResponse {
    pub session_id: String,
    pub target_turn: usize,
    pub restored_checkpoint_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CodebaseMapResponse {
    pub session_id: String,
    pub map: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InitProjectRequest {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InitProjectResponse {
    pub session_id: String,
    pub path: String,
    pub created: bool,
    pub overwritten: bool,
    pub detected_type: String,
    pub message: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct McpServerStatus {
    pub name: String,
    pub command: String,
    pub status: String,
    pub tools: Vec<McpToolInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct McpListResponse {
    pub servers: Vec<McpServerStatus>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CompactRequest {
    #[serde(default)]
    pub focus: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CompactResponse {
    pub session_id: String,
    pub original_messages: usize,
    pub compacted_messages: usize,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FileDiffStatus {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FileDiffEntry {
    pub path: String,
    pub status: FileDiffStatus,
    /// `None` for binary files.
    pub insertions: Option<usize>,
    pub deletions: Option<usize>,
    pub binary: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FileDiffListResponse {
    pub session_id: String,
    pub files: Vec<FileDiffEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FileDiffResponse {
    pub session_id: String,
    pub path: String,
    pub diff: String,
    /// True when `diff` was cut to the server's size cap.
    pub truncated: bool,
}

/// Cross-session activity for dashboards (`GET /api/v1/events`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GlobalEvent {
    SessionCreated {
        session_id: SessionId,
    },
    /// Title, model or worktree metadata changed.
    SessionUpdated {
        session_id: SessionId,
    },
    SessionDeleted {
        session_id: SessionId,
    },
    TurnStarted {
        session_id: SessionId,
    },
    TurnFinished {
        session_id: SessionId,
        reason: StopReason,
        message: Option<String>,
    },
    PermissionRequested {
        session_id: SessionId,
        request: PermissionRequest,
    },
    PermissionResolved {
        session_id: SessionId,
        request_id: String,
    },
}

pub use crate::command::QueueSteeringPrompt;
