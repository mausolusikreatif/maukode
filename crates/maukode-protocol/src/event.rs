use crate::message::{ToolCall, ToolResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Error,
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
    PermissionRequest(PermissionRequest),
    Usage(TokenUsage),
    ContextWarning {
        current_tokens: usize,
        max_tokens: usize,
        percent: u32,
        message: String,
    },
    Finish {
        reason: FinishReason,
    },
    Error {
        message: String,
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
