use super::protocol::*;
use crate::config::McpServerConfig;
use anyhow::{anyhow, Context, Result};
use maukode_tools::{sandboxed_command, SandboxMode};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, Mutex};

type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>>;

/// The answer to a request the server sent us. Only `ping` is supported: this client offers no
/// other capabilities (sampling, roots, elicitation).
fn server_request_reply(id: &serde_json::Value, method: &str) -> serde_json::Value {
    if method == "ping" {
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": {} })
    } else {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") },
        })
    }
}

pub struct McpClient {
    name: String,
    /// Identifies the command this client was started with (see [`McpClient::signature_of`]).
    signature: String,
    sandbox: SandboxMode,
    stdin_tx: mpsc::Sender<String>,
    pending_requests: PendingRequests,
    next_id: AtomicU64,
    child: Arc<Mutex<Option<Child>>>,
    /// The server's tool list, read on every prompt. Fetched once and again only after the
    /// server sends `notifications/tools/list_changed`.
    tools: Mutex<Option<Vec<McpToolDefinition>>>,
    tools_stale: Arc<AtomicBool>,
}

impl McpClient {
    /// Fingerprint of what a server runs (command and arguments, not secrets in `env`), so a
    /// cached connection is only reused for an identical definition.
    pub fn signature_of(config: &McpServerConfig) -> String {
        std::iter::once(config.command.as_str())
            .chain(config.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\0")
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn sandbox(&self) -> SandboxMode {
        self.sandbox
    }

    pub async fn connect(
        name: impl Into<String>,
        config: &McpServerConfig,
        cwd: &Path,
        sandbox: SandboxMode,
    ) -> Result<Self> {
        let name = name.into();
        let mut cmd = sandboxed_command(&config.command, &config.args, cwd, sandbox)
            .map_err(|e| anyhow!(e))?;
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().with_context(|| {
            format!("Failed to spawn MCP server '{}' ({})", name, config.command)
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Child stdin not piped for MCP server '{name}'"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Child stdout not piped for MCP server '{name}'"))?;
        let stderr = child.stderr.take();

        // 1. Drain stderr in background to prevent child process deadlocks
        if let Some(err) = stderr {
            let s_name = name.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    tracing::debug!("[MCP:{}:stderr] {}", s_name, line);
                }
            });
        }

        // 2. Setup async stdin writer queue
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(64);
        tokio::spawn(async move {
            let mut writer = stdin;
            while let Some(msg) = stdin_rx.recv().await {
                if writer.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        // 3. Setup response dispatching from stdout
        let pending_requests: PendingRequests = Arc::new(Mutex::new(HashMap::new()));
        let pending_map = Arc::clone(&pending_requests);
        let s_name = name.clone();
        let tools_stale = Arc::new(AtomicBool::new(false));
        let stale_flag = Arc::clone(&tools_stale);
        let reply_tx = stdin_tx.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    // Requests and notifications from the server carry a method; responses do not.
                    if let Some(method) = val.get("method").and_then(|m| m.as_str()) {
                        match val.get("id") {
                            Some(id) => {
                                let reply = server_request_reply(id, method);
                                let _ = reply_tx.send(reply.to_string()).await;
                            }
                            None if method == "notifications/tools/list_changed" => {
                                stale_flag.store(true, Ordering::Release);
                            }
                            None => {
                                tracing::trace!("[MCP:{}:notification] {}", s_name, trimmed);
                            }
                        }
                    } else if let Some(id) = val.get("id").and_then(|v| v.as_u64()) {
                        let sender = {
                            let mut map = pending_map.lock().await;
                            map.remove(&id)
                        };

                        if let Some(tx) = sender {
                            if let Some(err) = val.get("error") {
                                let msg = err
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("Unknown JSON-RPC error");
                                let _ = tx.send(Err(msg.to_string()));
                            } else if let Some(res) = val.get("result") {
                                let _ = tx.send(Ok(res.clone()));
                            } else {
                                let _ = tx.send(Ok(serde_json::Value::Null));
                            }
                        }
                    }
                }
            }
        });

        let client = Self {
            name,
            stdin_tx,
            pending_requests,
            next_id: AtomicU64::new(1),
            child: Arc::new(Mutex::new(Some(child))),
            signature: Self::signature_of(config),
            sandbox,
            tools: Mutex::new(None),
            tools_stale,
        };

        // 4. Perform MCP initialize handshake
        client.initialize().await?;

        Ok(client)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Send a JSON-RPC request and wait for the response
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout_dur: Duration,
    ) -> Result<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();

        {
            let mut map = self.pending_requests.lock().await;
            map.insert(id, tx);
        }

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.stdin_tx
            .send(payload.to_string())
            .await
            .map_err(|e| anyhow!("Failed to send request to MCP server '{}': {e}", self.name))?;

        match tokio::time::timeout(timeout_dur, rx).await {
            Ok(Ok(res)) => res.map_err(|e| anyhow!("MCP error from '{}': {e}", self.name)),
            Ok(Err(_)) => Err(anyhow!("MCP server '{}' closed connection", self.name)),
            Err(_) => {
                let mut map = self.pending_requests.lock().await;
                map.remove(&id);
                Err(anyhow!(
                    "MCP request '{method}' to '{}' timed out after {:?}",
                    self.name,
                    timeout_dur
                ))
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected)
    pub async fn send_notification(&self, method: &str, params: serde_json::Value) -> Result<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        self.stdin_tx.send(payload.to_string()).await.map_err(|e| {
            anyhow!(
                "Failed to send notification to MCP server '{}': {e}",
                self.name
            )
        })?;

        Ok(())
    }

    /// Perform MCP protocol initialization handshake
    async fn initialize(&self) -> Result<InitializeResult> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": { "listChanged": true }
            },
            "clientInfo": {
                "name": "maukode",
                "version": "0.1.0"
            }
        });

        let resp_val = self
            .send_request("initialize", params, Duration::from_secs(15))
            .await
            .context("MCP initialize failed")?;

        let init_result: InitializeResult = serde_json::from_value(resp_val)
            .context("Invalid initialize response from MCP server")?;

        // Send notifications/initialized as required by MCP spec
        self.send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        Ok(init_result)
    }

    /// The server's tools: the cached list, fetched again after the server reported a change.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let mut cached = self.tools.lock().await;
        // Cleared before fetching, so a change reported during the fetch triggers another one.
        let stale = self.tools_stale.swap(false, Ordering::AcqRel);
        if let Some(tools) = cached.as_ref().filter(|_| !stale) {
            return Ok(tools.clone());
        }
        let tools = match self.fetch_tools().await {
            Ok(tools) => tools,
            Err(error) => {
                if stale {
                    self.tools_stale.store(true, Ordering::Release);
                }
                return Err(error);
            }
        };
        *cached = Some(tools.clone());
        Ok(tools)
    }

    async fn fetch_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let resp_val = self
            .send_request("tools/list", serde_json::json!({}), Duration::from_secs(20))
            .await
            .context("MCP tools/list failed")?;

        let list_res: ToolsListResult = serde_json::from_value(resp_val)
            .context("Invalid tools/list response format from MCP server")?;

        Ok(list_res.tools)
    }

    /// Invoke a tool on the MCP server via tools/call
    pub async fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let params = serde_json::json!({
            "name": name,
            "arguments": args
        });

        let resp_val = self
            .send_request("tools/call", params, Duration::from_secs(60))
            .await
            .with_context(|| format!("MCP tool call '{name}' failed"))?;

        let call_res: CallToolResult = serde_json::from_value(resp_val)
            .context("Invalid tools/call result format from MCP server")?;

        let mut output = String::new();
        for item in &call_res.content {
            if let Some(ref text) = item.text {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(text);
            }
        }

        if call_res.is_error {
            Err(anyhow!(
                "MCP tool '{name}' returned error: {}",
                if output.is_empty() {
                    "Execution error".to_string()
                } else {
                    output
                }
            ))
        } else {
            Ok(output)
        }
    }

    /// Gracefully shutdown the server process
    pub async fn shutdown(&self) {
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
        }
    }
}
