pub mod client;
pub mod protocol;
pub mod tool;

pub use client::McpClient;
pub use tool::McpTool;

use crate::config::{AppConfig, McpServerConfig};
use maukode_protocol::{McpServerStatus, McpToolInfo};
use maukode_tools::SandboxMode;
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

        // 2. Project-local .maukode/mcp.json or mcp.json. These start programs named by the
        //    repository itself, so they only load for projects the user trusts.
        let local_candidates = [cwd.join(".maukode").join("mcp.json"), cwd.join("mcp.json")];
        let trusted = app_config.security.is_trusted_project(cwd)
            || AppConfig::is_trusted_project_on_disk(cwd);
        if !trusted {
            if local_candidates.iter().any(|path| path.is_file()) {
                tracing::warn!(
                    project = %cwd.display(),
                    "Ignoring this project's MCP servers because the folder is not trusted; \
                     run `maukode trust` in it to allow them"
                );
            }
            return combined;
        }

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
        sandbox: SandboxMode,
    ) -> anyhow::Result<Arc<McpClient>> {
        {
            let guard = self.clients.read().await;
            if let Some(client) = guard.get(name) {
                if client.sandbox() == sandbox {
                    return Ok(Arc::clone(client));
                }
            }
        }

        let client = Arc::new(McpClient::connect(name, config, cwd, sandbox).await?);
        let mut guard = self.clients.write().await;
        if let Some(previous) = guard.insert(name.to_string(), Arc::clone(&client)) {
            previous.shutdown().await;
        }
        Ok(client)
    }

    /// Discover tools from all configured MCP servers and register them dynamically
    pub async fn register_tools_into_registry(
        &self,
        registry: &mut ToolRegistry,
        cwd: &Path,
        app_config: &AppConfig,
    ) {
        self.register_scoped_tools_into_registry(registry, cwd, app_config, None, None)
            .await;
    }

    /// Discover tools from allowed MCP servers and register matching tools dynamically
    pub async fn register_scoped_tools_into_registry(
        &self,
        registry: &mut ToolRegistry,
        cwd: &Path,
        app_config: &AppConfig,
        allowed_servers: Option<&[String]>,
        allowed_tools: Option<&[String]>,
    ) {
        let configs = Self::load_server_configs(cwd, app_config).await;
        if configs.is_empty() {
            return;
        }

        for (name, config) in configs {
            if let Some(servers) = allowed_servers {
                if !servers.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                    continue;
                }
            }

            match self
                .get_or_connect(&name, &config, cwd, app_config.security.sandbox_mode)
                .await
            {
                Ok(client) => match client.list_tools().await {
                    Ok(tools) => {
                        let mut registered_count = 0;
                        for t in tools {
                            let full_tool_name = format!("mcp__{}__{}", name, t.name);
                            if let Some(tool_filter) = allowed_tools {
                                if !tool_filter.iter().any(|allowed| {
                                    allowed.eq_ignore_ascii_case(&t.name)
                                        || allowed.eq_ignore_ascii_case(&full_tool_name)
                                }) {
                                    continue;
                                }
                            }

                            let read_only = t.is_read_only();
                            let tool = McpTool::new(
                                &name,
                                t.name,
                                t.description,
                                t.input_schema,
                                Arc::clone(&client),
                            )
                            .with_read_only(read_only);
                            registry.register(Arc::new(tool));
                            registered_count += 1;
                        }
                        tracing::info!(
                            "Registered {registered_count} tools from MCP server '{name}'"
                        );
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
            match self
                .get_or_connect(&name, &config, cwd, app_config.security.sandbox_mode)
                .await
            {
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

    /// Report configured servers **without starting any**. Only servers that an agent turn has
    /// already connected are queried for their tools; the rest are `not_started`. Unlike
    /// [`Self::list_server_statuses`] this has no side effects, so it is safe behind a plain GET.
    pub async fn list_configured_statuses(
        &self,
        cwd: &Path,
        app_config: &AppConfig,
    ) -> Vec<McpServerStatus> {
        let configs = Self::load_server_configs(cwd, app_config).await;
        let mut statuses = Vec::new();
        for (name, config) in configs {
            let command = format!("{} {}", config.command, config.args.join(" "))
                .trim()
                .to_string();
            let connected = self.clients.read().await.get(&name).cloned();
            // A client started from a different command (config edited since) is stale: report
            // it as not started rather than showing the old server's tools under the new config.
            let connected = connected.filter(|c| c.signature() == McpClient::signature_of(&config));
            let (status, tools, error) = match connected {
                Some(client) => match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    client.list_tools(),
                )
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("timed out")))
                {
                    Ok(tools) => (
                        "connected",
                        tools
                            .into_iter()
                            .map(|t| McpToolInfo {
                                name: format!("mcp__{}__{}", name, t.name),
                                description: t.description.unwrap_or_default(),
                            })
                            .collect(),
                        None,
                    ),
                    Err(e) => ("error", Vec::new(), Some(format!("tools/list failed: {e}"))),
                },
                None => ("not_started", Vec::new(), None),
            };
            statuses.push(McpServerStatus {
                name,
                command,
                status: status.to_string(),
                tools,
                error,
            });
        }
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
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

        // An untrusted project's own servers are ignored.
        let loaded = McpManager::load_server_configs(cwd, &app_config).await;
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key("srv_a"));

        app_config.security.trusted_projects = vec![cwd.to_path_buf()];
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

        let client = McpClient::connect("mock", &cfg, cwd, SandboxMode::Off)
            .await
            .unwrap();
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

    #[tokio::test]
    async fn tool_list_is_cached_until_the_server_reports_a_change() {
        let dir = tempdir().unwrap();
        let cwd = dir.path();

        // Pings the client during the handshake, lists one more tool after every change, and
        // reports a change whenever a tool is called.
        let mock_server_py = r#"
import sys, json

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

lists = 0
pinged = False
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method")
    msg_id = msg.get("id")
    if method is None:
        if msg_id == "srv-1" and msg.get("result") == {}:
            pinged = True
        continue
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": "srv-1", "method": "ping"})
        send({"jsonrpc": "2.0", "id": msg_id, "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {"listChanged": True}},
            "serverInfo": {"name": "mock-mcp", "version": "1.0"}}})
    elif method == "tools/list":
        lists += 1
        names = ["tool%d" % i for i in range(lists)]
        if pinged:
            names.append("pinged")
        send({"jsonrpc": "2.0", "id": msg_id, "result": {
            "tools": [{"name": n, "inputSchema": {"type": "object"}} for n in names]}})
    elif method == "tools/call":
        send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
        send({"jsonrpc": "2.0", "id": msg_id, "result": {
            "content": [{"type": "text", "text": "ok"}], "isError": False}})
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
        let client = McpClient::connect("mock", &cfg, cwd, SandboxMode::Off)
            .await
            .unwrap();
        let names = |tools: Vec<protocol::McpToolDefinition>| -> Vec<String> {
            tools.into_iter().map(|t| t.name).collect()
        };

        // The ping was answered before the first listing.
        assert_eq!(
            names(client.list_tools().await.unwrap()),
            ["tool0", "pinged"]
        );
        // No change reported: served from the cache, not listed again.
        assert_eq!(
            names(client.list_tools().await.unwrap()),
            ["tool0", "pinged"]
        );

        client
            .call_tool("tool0", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(
            names(client.list_tools().await.unwrap()),
            ["tool0", "tool1", "pinged"]
        );
    }
}
