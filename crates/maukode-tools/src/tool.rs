use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Where a tool comes from. Lets the registry expose or gate whole groups of tools (native
/// built-ins, in-process memory tools, sandbox tools, or external MCP tools) without the runner
/// special-casing each source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    #[default]
    Native,
    Memory,
    Sandbox,
    Mcp,
}

impl ToolSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolSource::Native => "native",
            ToolSource::Memory => "memory",
            ToolSource::Sandbox => "sandbox",
            ToolSource::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Tool not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    /// Origin of the tool, so callers can gate exposure by source group.
    #[serde(default)]
    pub source: ToolSource,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;

    /// Origin of this tool. Defaults to [`ToolSource::Native`]; MCP, memory, and sandbox tools
    /// override it so the registry can treat each group consistently.
    fn source(&self) -> ToolSource {
        ToolSource::Native
    }

    /// Whether `ask` mode confirms every call of this tool with the user. Native tools rely on the
    /// path jail and the bash risk assessment instead; MCP tools run code MauKode cannot inspect.
    fn requires_confirmation(&self) -> bool {
        false
    }

    /// Rough size of this tool's schema/description in tokens. Used by the registry to reason about
    /// how much prompt budget a tool group consumes; `None` means "unknown/negligible".
    fn token_cost(&self) -> Option<usize> {
        None
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
            source: self.source(),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError>;
}
