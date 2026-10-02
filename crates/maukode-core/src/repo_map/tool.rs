use super::RepoMapGenerator;
use async_trait::async_trait;
use maukode_tools::{Tool, ToolError};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub struct CodebaseMapTool {
    generator: Arc<RepoMapGenerator>,
}

impl CodebaseMapTool {
    pub fn new(generator: Arc<RepoMapGenerator>) -> Self {
        Self { generator }
    }
}

#[derive(Deserialize)]
struct CodebaseMapArgs {
    subpath: Option<String>,
    query: Option<String>,
    max_tokens: Option<usize>,
}

#[async_trait]
impl Tool for CodebaseMapTool {
    fn name(&self) -> &str {
        "get_codebase_map"
    }

    fn description(&self) -> &str {
        "Inspect a compact Tree-sitter AST symbol map of the codebase (structs, classes, functions, traits, interfaces). Can be focused on a subpath or filtered with a query keyword."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "subpath": {
                    "type": "string",
                    "description": "Optional subdirectory or module path to focus the map on (e.g. 'crates/maukode-core')"
                },
                "query": {
                    "type": "string",
                    "description": "Optional search term or symbol name to prioritize in the map"
                },
                "max_tokens": {
                    "type": "integer",
                    "description": "Maximum token budget for the skeleton map (default: 1000)"
                }
            }
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, ToolError> {
        let args: CodebaseMapArgs = serde_json::from_value(arguments)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;
        let tokens = args.max_tokens.unwrap_or(1000);
        let map = self
            .generator
            .generate_map(tokens, args.subpath.as_deref(), args.query.as_deref())
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        if map.trim().is_empty() {
            Ok("No symbols found matching the specified path/query.".to_string())
        } else {
            Ok(map)
        }
    }
}
