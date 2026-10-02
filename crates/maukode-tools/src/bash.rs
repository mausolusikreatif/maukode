use crate::security::PathJail;
use crate::tool::{Tool, ToolError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

pub struct BashTool {
    pub default_cwd: PathBuf,
    pub jail: PathJail,
    pub default_timeout: Duration,
}

impl BashTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let dir = cwd.into();
        Self {
            jail: PathJail::new(&dir),
            default_cwd: dir,
            default_timeout: Duration::from_secs(120),
        }
    }

    pub fn with_jail(cwd: impl Into<PathBuf>, jail: PathJail) -> Self {
        Self {
            default_cwd: cwd.into(),
            jail,
            default_timeout: Duration::from_secs(120),
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

        let child = Command::new("bash")
            .arg("-c")
            .arg(&args.command)
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::Execution(format!("Failed to spawn bash process: {e}")))?;

        let execution = async {
            let output = child
                .wait_with_output()
                .await
                .map_err(|e| ToolError::Execution(format!("Command execution failed: {e}")))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            let mut result = String::new();
            if !stdout.is_empty() {
                result.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str("[stderr]\n");
                result.push_str(&stderr);
            }
            if exit_code != 0 {
                result.push_str(&format!("\n[Process exited with code {exit_code}]"));
            }
            if result.trim().is_empty() {
                result.push_str("(Command finished with no output)");
            }
            Ok(result)
        };

        tokio::time::timeout(timeout_dur, execution)
            .await
            .map_err(|_| {
                ToolError::Execution(format!(
                    "Command timed out after {} seconds",
                    timeout_dur.as_secs()
                ))
            })?
    }
}
