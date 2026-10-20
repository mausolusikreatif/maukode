//! Hooks: shell commands that run around tool calls and at the end of a turn.
//!
//! - `pre_tool_use` runs before a matching tool call; a non-zero exit blocks the call, and the
//!   model receives the hook's output as the tool's error.
//! - `post_tool_use` runs after a matching call (for example a formatter after edits); when it
//!   fails, its output is appended to the tool result so the model can react.
//! - `turn_end` runs after every turn; failures are only logged.
//!
//! Hooks run on the host with `bash -c` in the session's directory, whatever execution backend
//! the session uses. They receive a JSON description of the event on stdin and in
//! `MAUKODE_HOOK_EVENT`, `MAUKODE_SESSION_ID` and `MAUKODE_TOOL_NAME`. Hooks come from the global
//! configuration and, for trusted projects only, from the project's `.maukode/hooks.json`.

use crate::config::AppConfig;
use maukode_protocol::{StopReason, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Hook output handed to the model is cut to this many characters.
const MAX_OUTPUT_CHARS: usize = 4000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_tool_use: Vec<Hook>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_tool_use: Vec<Hook>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_end: Vec<Hook>,
}

impl HooksConfig {
    pub fn is_empty(&self) -> bool {
        self.pre_tool_use.is_empty() && self.post_tool_use.is_empty() && self.turn_end.is_empty()
    }

    fn extend(&mut self, other: HooksConfig) {
        self.pre_tool_use.extend(other.pre_tool_use);
        self.post_tool_use.extend(other.post_tool_use);
        self.turn_end.extend(other.turn_end);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hook {
    /// Tool names (`*` wildcards allowed) the hook applies to; empty means every tool. Ignored
    /// for `turn_end`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Shell command, run with `bash -c`.
    pub command: String,
    /// Seconds before the hook is stopped (default 60). A `pre_tool_use` hook that times out
    /// blocks the call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

impl Hook {
    fn applies_to(&self, tool_name: &str) -> bool {
        self.tools.is_empty()
            || self
                .tools
                .iter()
                .any(|pattern| crate::permission::glob_match(pattern, tool_name))
    }
}

/// The hooks of one workspace.
#[derive(Debug, Clone, Default)]
pub struct Hooks {
    config: HooksConfig,
    cwd: PathBuf,
}

struct Outcome {
    success: bool,
    output: String,
}

impl Hooks {
    pub fn new(config: HooksConfig, cwd: impl Into<PathBuf>) -> Self {
        Self {
            config,
            cwd: cwd.into(),
        }
    }

    /// Global hooks, then the project's `.maukode/hooks.json` when the project is trusted.
    pub fn load(cwd: &Path, app_config: &AppConfig) -> Self {
        let mut config = app_config.hooks.clone();
        let trusted = app_config.security.is_trusted_project(cwd)
            || AppConfig::is_trusted_project_on_disk(cwd);
        if trusted {
            let path = cwd.join(".maukode").join("hooks.json");
            if let Ok(text) = std::fs::read_to_string(&path) {
                match serde_json::from_str::<HooksConfig>(&text) {
                    Ok(project) => config.extend(project),
                    Err(error) => {
                        tracing::warn!(%error, path = %path.display(), "Ignoring invalid hooks file")
                    }
                }
            }
        }
        Self::new(config, cwd)
    }

    pub fn is_empty(&self) -> bool {
        self.config.is_empty()
    }

    /// Whether any `pre_tool_use` hook applies to `tool_name`.
    pub fn has_pre_tool_use(&self, tool_name: &str) -> bool {
        self.config
            .pre_tool_use
            .iter()
            .any(|hook| hook.applies_to(tool_name))
    }

    /// Run the `pre_tool_use` hooks for `call`. `Err` carries the reason the call is blocked.
    pub async fn before_tool(&self, session_id: &str, call: &ToolCall) -> Result<(), String> {
        let event = json!({
            "event": "pre_tool_use",
            "session_id": session_id,
            "tool_name": call.name,
            "tool_input": call.arguments,
        });
        for hook in self
            .config
            .pre_tool_use
            .iter()
            .filter(|h| h.applies_to(&call.name))
        {
            let outcome = self.run(hook, &event, session_id, Some(&call.name)).await;
            if !outcome.success {
                return Err(format!(
                    "Blocked by the pre_tool_use hook `{}`:\n{}",
                    hook.command, outcome.output
                ));
            }
        }
        Ok(())
    }

    /// Run the `post_tool_use` hooks for `call`. Returns what failing hooks printed, to append to
    /// the tool result.
    pub async fn after_tool(
        &self,
        session_id: &str,
        call: &ToolCall,
        output: &str,
        is_error: bool,
    ) -> Option<String> {
        let event = json!({
            "event": "post_tool_use",
            "session_id": session_id,
            "tool_name": call.name,
            "tool_input": call.arguments,
            "tool_output": truncate(output),
            "is_error": is_error,
        });
        let mut feedback = Vec::new();
        for hook in self
            .config
            .post_tool_use
            .iter()
            .filter(|h| h.applies_to(&call.name))
        {
            let outcome = self.run(hook, &event, session_id, Some(&call.name)).await;
            if !outcome.success {
                feedback.push(format!(
                    "The post_tool_use hook `{}` failed:\n{}",
                    hook.command, outcome.output
                ));
            }
        }
        (!feedback.is_empty()).then(|| feedback.join("\n\n"))
    }

    /// Run the `turn_end` hooks.
    pub async fn turn_end(&self, session_id: &str, reason: StopReason) {
        let event = json!({
            "event": "turn_end",
            "session_id": session_id,
            "stop_reason": reason,
        });
        for hook in &self.config.turn_end {
            let outcome = self.run(hook, &event, session_id, None).await;
            if !outcome.success {
                tracing::warn!(command = %hook.command, output = %outcome.output, "turn_end hook failed");
            }
        }
    }

    async fn run(
        &self,
        hook: &Hook,
        event: &Value,
        session_id: &str,
        tool_name: Option<&str>,
    ) -> Outcome {
        let mut command = tokio::process::Command::new("bash");
        command
            .arg("-c")
            .arg(&hook.command)
            .current_dir(&self.cwd)
            .env(
                "MAUKODE_HOOK_EVENT",
                event["event"].as_str().unwrap_or_default(),
            )
            .env("MAUKODE_SESSION_ID", session_id)
            .env("MAUKODE_TOOL_NAME", tool_name.unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                return Outcome {
                    success: false,
                    output: format!("could not start: {e}"),
                }
            }
        };
        // Written alongside, so a hook that never reads stdin cannot stall past its timeout.
        if let Some(mut stdin) = child.stdin.take() {
            let payload = event.to_string();
            tokio::spawn(async move {
                let _ = stdin.write_all(payload.as_bytes()).await;
            });
        }
        let timeout = Duration::from_secs(hook.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));
        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => {
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                let mut output = truncate(text.trim());
                if !out.status.success() {
                    let code = out
                        .status
                        .code()
                        .map_or("a signal".to_string(), |c| format!("code {c}"));
                    output = format!("{output}\n(exited with {code})").trim().to_string();
                }
                Outcome {
                    success: out.status.success(),
                    output,
                }
            }
            Ok(Err(e)) => Outcome {
                success: false,
                output: format!("failed: {e}"),
            },
            Err(_) => Outcome {
                success: false,
                output: format!("timed out after {}s", timeout.as_secs()),
            },
        }
    }
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_OUTPUT_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_OUTPUT_CHARS).collect();
    format!("{cut}\n… (truncated)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(tools: &[&str], command: &str) -> Hook {
        Hook {
            tools: tools.iter().map(|t| t.to_string()).collect(),
            command: command.into(),
            timeout_secs: None,
        }
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: name.into(),
            arguments: json!({ "path": "a.rs" }),
        }
    }

    #[tokio::test]
    async fn pre_hooks_block_on_failure_and_see_the_call() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = Hooks::new(
            HooksConfig {
                pre_tool_use: vec![
                    hook(&["bash"], "echo no bash; exit 1"),
                    // The event arrives on stdin.
                    hook(
                        &["write_*"],
                        "grep -q '\"path\":\"a.rs\"' && test \"$MAUKODE_TOOL_NAME\" = write_file",
                    ),
                ],
                ..HooksConfig::default()
            },
            dir.path(),
        );
        let blocked = hooks.before_tool("s", &call("bash")).await.unwrap_err();
        assert!(blocked.contains("no bash") && blocked.contains("exited with code 1"));
        assert!(hooks.before_tool("s", &call("write_file")).await.is_ok());
        assert!(hooks.before_tool("s", &call("read_file")).await.is_ok());
        assert!(hooks.has_pre_tool_use("bash") && !hooks.has_pre_tool_use("read_file"));
    }

    #[tokio::test]
    async fn post_hooks_report_failures_only() {
        let dir = tempfile::tempdir().unwrap();
        let hooks = Hooks::new(
            HooksConfig {
                post_tool_use: vec![
                    hook(&[], "touch ran"),
                    hook(&["edit_file"], "echo formatter broke >&2; exit 3"),
                ],
                ..HooksConfig::default()
            },
            dir.path(),
        );
        assert_eq!(
            hooks.after_tool("s", &call("read_file"), "ok", false).await,
            None
        );
        assert!(dir.path().join("ran").exists());
        let feedback = hooks
            .after_tool("s", &call("edit_file"), "ok", false)
            .await
            .unwrap();
        assert!(feedback.contains("formatter broke") && feedback.contains("code 3"));
    }

    #[tokio::test]
    async fn slow_hooks_time_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut slow = hook(&[], "sleep 5");
        slow.timeout_secs = Some(1);
        let hooks = Hooks::new(
            HooksConfig {
                pre_tool_use: vec![slow],
                ..HooksConfig::default()
            },
            dir.path(),
        );
        let blocked = hooks.before_tool("s", &call("bash")).await.unwrap_err();
        assert!(blocked.contains("timed out"));
    }

    #[test]
    fn project_hooks_need_a_trusted_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".maukode")).unwrap();
        std::fs::write(
            dir.path().join(".maukode/hooks.json"),
            r#"{"turn_end": [{"command": "true"}]}"#,
        )
        .unwrap();
        let mut config = AppConfig::default();
        config.hooks.post_tool_use.push(hook(&[], "true"));
        let untrusted = Hooks::load(dir.path(), &config);
        assert!(untrusted.config.turn_end.is_empty());
        assert_eq!(untrusted.config.post_tool_use.len(), 1);

        config.security.trusted_projects = vec![dir.path().to_path_buf()];
        let trusted = Hooks::load(dir.path(), &config);
        assert_eq!(trusted.config.turn_end.len(), 1);
    }
}
