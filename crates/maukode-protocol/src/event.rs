use crate::message::{ToolCall, ToolResult};
use crate::session::SessionId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowSession,
    /// Execute the tool locally, show the result to the user, and do not send the result to the
    /// model. The current agent turn stops after all pending tool calls have completed.
    AllowPrivate,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    ContextWarning {
        current_tokens: usize,
        max_tokens: usize,
        percent: u32,
        message: String,
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
pub struct DiffResponse {
    pub session_id: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub stat_summary: String,
    pub diff_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoResponse {
    pub session_id: String,
    pub target_turn: usize,
    pub restored_checkpoint_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodebaseMapResponse {
    pub session_id: String,
    pub map: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitProjectRequest {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub command: String,
    pub status: String,
    pub tools: Vec<McpToolInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpListResponse {
    pub servers: Vec<McpServerStatus>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactRequest {
    #[serde(default)]
    pub focus: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactResponse {
    pub session_id: String,
    pub original_messages: usize,
    pub compacted_messages: usize,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDiffStatus {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffEntry {
    pub path: String,
    pub status: FileDiffStatus,
    /// `None` for binary files.
    pub insertions: Option<usize>,
    pub deletions: Option<usize>,
    pub binary: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffListResponse {
    pub session_id: String,
    pub files: Vec<FileDiffEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffResponse {
    pub session_id: String,
    pub path: String,
    pub diff: String,
    /// True when `diff` was cut to the server's size cap.
    pub truncated: bool,
}

/// Cross-session activity for dashboards (`GET /api/v1/events`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
