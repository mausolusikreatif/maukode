use crate::compress::compress_output_in;
use crate::exec::{format_output, ExecBackend, ExecRequest, HostBackend};
use crate::security::{PathJail, SandboxMode};
use crate::tool::{Tool, ToolError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub struct BashTool {
    pub default_cwd: PathBuf,
    pub jail: PathJail,
    pub default_timeout: Duration,
    pub sandbox: SandboxMode,
    /// Where commands run: a local process by default, or a lawang cell.
    pub backend: Arc<dyn ExecBackend>,
}

impl BashTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let dir = cwd.into();
        Self {
            jail: PathJail::new(&dir),
            default_cwd: dir,
            default_timeout: Duration::from_secs(120),
            sandbox: SandboxMode::PathJail,
            backend: Arc::new(HostBackend::new(SandboxMode::PathJail)),
        }
    }

    pub fn with_jail(cwd: impl Into<PathBuf>, jail: PathJail) -> Self {
        Self {
            default_cwd: cwd.into(),
            jail,
            default_timeout: Duration::from_secs(120),
            sandbox: SandboxMode::PathJail,
            backend: Arc::new(HostBackend::new(SandboxMode::PathJail)),
        }
    }

    pub fn with_sandbox(cwd: impl Into<PathBuf>, jail: PathJail, sandbox: SandboxMode) -> Self {
        Self {
            default_cwd: cwd.into(),
            jail,
            default_timeout: Duration::from_secs(120),
            sandbox,
            backend: Arc::new(HostBackend::new(sandbox)),
        }
    }

    /// Run commands through `backend` instead of as local processes.
    pub fn with_backend(
        cwd: impl Into<PathBuf>,
        jail: PathJail,
        backend: Arc<dyn ExecBackend>,
    ) -> Self {
        Self {
            default_cwd: cwd.into(),
            jail,
            default_timeout: Duration::from_secs(120),
            sandbox: SandboxMode::Off,
            backend,
        }
    }
}

#[derive(Deserialize)]
struct BashArgs {
    command: String,
    cwd: Option<String>,
    timeout_seconds: Option<u64>,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a bash shell command in the project directory with a timeout."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command line string to execute in bash"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory for the command"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Optional timeout in seconds (default 120)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let args: BashArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

        let cwd = match args.cwd {
            Some(ref c) => self.jail.validate_path(c)?,
            None => self.default_cwd.clone(),
        };

        let timeout_dur = args
            .timeout_seconds
            .map(Duration::from_secs)
            .unwrap_or(self.default_timeout);

        // No sync before a command: the backend that runs it holds the truth. Changes made on the
        // host by the file tools were pushed to it when they were made (`SyncedTool`), and what
        // the command changes comes back before the host next reads (`before_workspace_access`).
        let command = args.command.clone();
        let output = self
            .backend
            .exec(ExecRequest {
                command: args.command,
                cwd,
                timeout: timeout_dur,
            })
            .await?;
        let text = format_output(&output);
        let project = &self.default_cwd;
        let compressed = compress_output_in(project, &command, output.exit_code, &text);
        Ok(compressed.unwrap_or(text))
    }
}
