use super::client::McpClient;
use async_trait::async_trait;
use maukode_tools::{Tool, ToolError};
use std::sync::Arc;

pub struct McpTool {
    _server_name: String,
    original_name: String,
    full_name: String,
    description: String,
    parameters: serde_json::Value,
    client: Arc<McpClient>,
}

impl McpTool {
    pub fn new(
        server_name: impl Into<String>,
        original_name: impl Into<String>,
        description: Option<String>,
        parameters: serde_json::Value,
        client: Arc<McpClient>,
    ) -> Self {
        let server_name = server_name.into();
        let original_name = original_name.into();
        let full_name = format!("mcp__{server_name}__{original_name}");
        let description = description.unwrap_or_else(|| {
            format!("External tool '{original_name}' provided by MCP server '{server_name}'")
        });

        Self {
            _server_name: server_name,
            original_name,
            full_name,
            description,
            parameters,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.full_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        self.client
            .call_tool(&self.original_name, args)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))
    }
}
