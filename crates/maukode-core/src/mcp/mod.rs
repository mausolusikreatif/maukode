pub mod client;
pub mod protocol;
pub mod tool;

pub use client::McpClient;
pub use tool::McpTool;

use crate::config::{AppConfig, McpServerConfig};
use maukode_protocol::{McpServerStatus, McpToolInfo};
use maukode_tools::ToolRegistry;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
struct McpConfigFile {
    #[serde(default, rename = "mcpServers", alias = "mcp_servers")]
    mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Default)]
pub struct McpManager {
    clients: Arc<RwLock<HashMap<String, Arc<McpClient>>>>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load combined MCP server definitions from global config and project-local files
    pub async fn load_server_configs(
        cwd: &Path,
        app_config: &AppConfig,
    ) -> HashMap<String, McpServerConfig> {
        let mut combined = app_config.mcp_servers.clone();

        // 1. Global ~/.config/maukode/mcp.json or ~/.maukode/mcp.json
        let mut global_candidates = Vec::new();
        if let Some(cfg_dir) = crate::paths::config_dir() {
            global_candidates.push(cfg_dir.join("maukode").join("mcp.json"));
        }
        if let Some(home_dir) = dirs::home_dir() {
            global_candidates.push(home_dir.join(".config").join("maukode").join("mcp.json"));
            global_candidates.push(home_dir.join(".maukode").join("mcp.json"));
        }

        for path in global_candidates {
            if path.is_file() {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(cfg) = serde_json::from_str::<McpConfigFile>(&content) {
                        for (k, v) in cfg.mcp_servers {
                            combined.insert(k, v);
                        }
                    }
                }
            }
        }

        // 2. Project-local .maukode/mcp.json or mcp.json
        let local_candidates = [cwd.join(".maukode").join("mcp.json"), cwd.join("mcp.json")];

        for path in local_candidates {
            if path.is_file() {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(cfg) = serde_json::from_str::<McpConfigFile>(&content) {
                        for (k, v) in cfg.mcp_servers {
                            combined.insert(k, v);
                        }
                    }
                }
            }
        }

        combined
    }

    /// Connect to an MCP server or return an existing active connection
    pub async fn get_or_connect(
        &self,
        name: &str,
        config: &McpServerConfig,
        cwd: &Path,
    ) -> anyhow::Result<Arc<McpClient>> {
        {
            let guard = self.clients.read().await;
            if let Some(client) = guard.get(name) {
                return Ok(Arc::clone(client));
            }
        }

        let client = Arc::new(McpClient::connect(name, config, cwd).await?);
        let mut guard = self.clients.write().await;
        guard.insert(name.to_string(), Arc::clone(&client));
        Ok(client)
    }

    /// Discover tools from all configured MCP servers and register them dynamically
    pub async fn register_tools_into_registry(
        &self,
        registry: &mut ToolRegistry,
        cwd: &Path,
        app_config: &AppConfig,
    ) {
        let configs = Self::load_server_configs(cwd, app_config).await;
        if configs.is_empty() {
            return;
        }

        for (name, config) in configs {
            match self.get_or_connect(&name, &config, cwd).await {
                Ok(client) => match client.list_tools().await {
                    Ok(tools) => {
                        let count = tools.len();
                        for t in tools {
                            let tool = McpTool::new(
                                &name,
                                t.name,
                                t.description,
                                t.input_schema,
                                Arc::clone(&client),
                            );
                            registry.register(Arc::new(tool));
                        }
                        tracing::info!("Registered {count} tools from MCP server '{name}'");
                    }
                    Err(e) => {
                        tracing::warn!("Failed to list tools from MCP server '{name}': {e}");
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to start MCP server '{name}': {e}");
                }
            }
        }
    }

    /// List active and configured MCP servers with status and tool counts
    pub async fn list_server_statuses(
        &self,
        cwd: &Path,
        app_config: &AppConfig,
    ) -> Vec<McpServerStatus> {
        let configs = Self::load_server_configs(cwd, app_config).await;
        let mut statuses = Vec::new();

        for (name, config) in configs {
            let cmd_str = format!("{} {}", config.command, config.args.join(" "))
                .trim()
                .to_string();
            match self.get_or_connect(&name, &config, cwd).await {
                Ok(client) => match client.list_tools().await {
                    Ok(tools) => {
                        let tool_infos = tools
                            .into_iter()
                            .map(|t| McpToolInfo {
                                name: format!("mcp__{}__{}", name, t.name),
                                description: t.description.unwrap_or_default(),
                            })
                            .collect();
                        statuses.push(McpServerStatus {
                            name,
                            command: cmd_str,
                            status: "connected".to_string(),
                            tools: tool_infos,
                            error: None,
                        });
                    }
                    Err(e) => {
                        statuses.push(McpServerStatus {
                            name,
                            command: cmd_str,
                            status: "error".to_string(),
                            tools: Vec::new(),
                            error: Some(format!("tools/list failed: {e}")),
                        });
                    }
                },
                Err(e) => {
                    statuses.push(McpServerStatus {
                        name,
                        command: cmd_str,
                        status: "disconnected".to_string(),
                        tools: Vec::new(),
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        statuses
    }

    /// Shutdown all running MCP server subprocesses
    pub async fn shutdown_all(&self) {
        let mut guard = self.clients.write().await;
        for (_, client) in guard.drain() {
            client.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_mcp_config_discovery_and_merging() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();

        // 1. AppConfig has server A
        let mut app_config = AppConfig::default();
        app_config.mcp_servers.insert(
            "srv_a".to_string(),
            McpServerConfig {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                env: HashMap::new(),
            },
        );

        // 2. Local .maukode/mcp.json has server B
        let maukode_dir = cwd.join(".maukode");
        tokio::fs::create_dir_all(&maukode_dir).await.unwrap();
        let local_json = r#"{
            "mcpServers": {
                "srv_b": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }"#;
        tokio::fs::write(maukode_dir.join("mcp.json"), local_json)
            .await
            .unwrap();

        let loaded = McpManager::load_server_configs(cwd, &app_config).await;
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains_key("srv_a"));
        assert!(loaded.contains_key("srv_b"));
        assert_eq!(loaded["srv_b"].command, "node");
    }

    #[tokio::test]
    async fn test_mcp_stdio_mock_server_handshake_and_tools() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();

        // Create a mock python script that acts as an MCP stdio server
        let mock_server_py = r#"
import sys, json

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    method = req.get("method")
    req_id = req.get("id")

    if method == "initialize":
        resp = {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": {"name": "mock-mcp", "version": "1.0"}
            }
        }
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        resp = {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "tools": [
                    {
                        "name": "calculate",
                        "description": "Perform calculation",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"expr": {"type": "string"}},
                            "required": ["expr"]
                        }
                    }
                ]
            }
        }
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()
    elif method == "tools/call":
        args = req.get("params", {}).get("arguments", {})
        expr = args.get("expr", "")
        resp = {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [{"type": "text", "text": f"Result of {expr} is 42"}],
                "isError": False
            }
        }
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()
"#;
        let script_path = cwd.join("mock_mcp.py");
        tokio::fs::write(&script_path, mock_server_py)
            .await
            .unwrap();

        let cfg = McpServerConfig {
            command: "python3".to_string(),
            args: vec![script_path.display().to_string()],
            env: HashMap::new(),
        };

        let client = McpClient::connect("mock", &cfg, cwd).await.unwrap();
        assert_eq!(client.name(), "mock");

        // Test tools/list
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "calculate");

        // Test tools/call
        let res = client
            .call_tool("calculate", serde_json::json!({"expr": "6 * 7"}))
            .await
            .unwrap();
        assert!(res.contains("Result of 6 * 7 is 42"));

        // Test McpTool adapter
        let tool = McpTool::new(
            "mock",
            tools[0].name.clone(),
            tools[0].description.clone(),
            tools[0].input_schema.clone(),
            Arc::new(client),
        );
        use maukode_tools::Tool;
        assert_eq!(tool.name(), "mcp__mock__calculate");
        let exec_res = tool
            .execute(serde_json::json!({"expr": "10 + 32"}))
            .await
            .unwrap();
        assert!(exec_res.contains("Result of 10 + 32 is 42"));
    }
}
