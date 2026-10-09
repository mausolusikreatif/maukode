use crate::tool::ToolError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;

/// Process isolation policy for command-like tools. `PathJail` is the compatibility default;
/// `Bubblewrap` is opt-in and only selected when the host provides bubblewrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    Off,
    #[default]
    PathJail,
    Bubblewrap,
    Auto,
}

impl SandboxMode {
    pub fn available() -> bool {
        cfg!(target_os = "linux") && which::which("bwrap").is_ok()
    }

    pub fn effective(self) -> Self {
        match self {
            Self::Auto if Self::available() => Self::Bubblewrap,
            Self::Auto => Self::PathJail,
            mode => mode,
        }
    }
}

/// Build a command using the selected process isolation policy.  The workspace is
/// mounted at `/workspace` in bubblewrap mode so that the child sees a stable cwd.
pub fn sandboxed_command(
    program: &str,
    args: &[String],
    cwd: &Path,
    mode: SandboxMode,
) -> Result<Command, String> {
    match mode.effective() {
        SandboxMode::Bubblewrap => {
            if !SandboxMode::available() {
                return Err("bubblewrap sandbox requested but bwrap is not installed".into());
            }
            let mut command = Command::new("bwrap");
            command.args([
                "--die-with-parent",
                "--new-session",
                "--unshare-pid",
                "--unshare-uts",
                "--ro-bind",
                "/usr",
                "/usr",
                "--ro-bind",
                "/bin",
                "/bin",
                "--ro-bind",
                "/lib",
                "/lib",
                "--ro-bind",
                "/lib64",
                "/lib64",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--bind",
                cwd.to_string_lossy().as_ref(),
                "/workspace",
                "--chdir",
                "/workspace",
                "--",
            ]);
            command.arg(program).args(args);
            Ok(command)
        }
        SandboxMode::Off | SandboxMode::PathJail | SandboxMode::Auto => {
            let mut command = Command::new(program);
            command.args(args).current_dir(cwd);
            Ok(command)
        }
    }
}

#[cfg(test)]
mod sandbox_tests {
    use super::SandboxMode;

    #[test]
    fn default_preserves_existing_path_jail_behavior() {
        assert_eq!(SandboxMode::default(), SandboxMode::PathJail);
    }

    #[test]
    fn explicit_modes_are_not_changed() {
        assert_eq!(SandboxMode::Off.effective(), SandboxMode::Off);
        assert_eq!(SandboxMode::PathJail.effective(), SandboxMode::PathJail);
        assert_eq!(SandboxMode::Bubblewrap.effective(), SandboxMode::Bubblewrap);
    }
}

#[derive(Debug, Clone)]
pub struct PathJail {
    pub workspace_root: PathBuf,
    pub canonical_root: PathBuf,
    pub allowed_external_paths: Vec<PathBuf>,
}

impl PathJail {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let root = workspace_root.into();
        let canonical_root = root
            .canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self {
            workspace_root: root,
            canonical_root,
            allowed_external_paths: Vec::new(),
        }
    }

    pub fn with_allowed_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.allowed_external_paths = paths
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();
        self
    }

    /// Resolve and validate that a requested path does not escape the workspace jail.
    pub fn validate_path(&self, requested: impl AsRef<Path>) -> Result<PathBuf, ToolError> {
        let req = requested.as_ref();

        // 1. Check for suspicious null bytes
        for comp in req.components() {
            if let Component::Normal(os_str) = comp {
                let s = os_str.to_string_lossy();
                if s.contains('\0') {
                    return Err(ToolError::PermissionDenied("Null byte in path".to_string()));
                }
            }
        }

        // 2. Resolve relative to workspace root
        let full_path = if req.is_absolute() {
            req.to_path_buf()
        } else {
            self.workspace_root.join(req)
        };

        // 3. If file or directory already exists, canonicalize directly
        if full_path.exists() {
            let canonical = full_path
                .canonicalize()
                .map_err(|e| ToolError::Execution(format!("Cannot resolve path: {e}")))?;

            if self.is_in_jail(&canonical) {
                Ok(canonical)
            } else {
                Err(ToolError::PermissionDenied(format!(
                    "Path traversal blocked: target '{}' escapes workspace jail '{}'",
                    canonical.display(),
                    self.workspace_root.display()
                )))
            }
        } else {
            // 4. If path does not exist (e.g. creating new file in write_file),
            // find the nearest existing ancestor, canonicalize it, and check jail containment.
            let mut ancestor = full_path.as_path();
            let mut remaining_parts = Vec::new();

            while !ancestor.exists() {
                if let Some(name) = ancestor.file_name() {
                    remaining_parts.push(name.to_os_string());
                }
                match ancestor.parent() {
                    Some(parent) => ancestor = parent,
                    None => break,
                }
            }

            let canonical_ancestor = if ancestor.exists() {
                ancestor.canonicalize().map_err(|e| {
                    ToolError::Execution(format!("Cannot resolve ancestor path: {e}"))
                })?
            } else {
                self.canonical_root.clone()
            };

            if !self.is_in_jail(&canonical_ancestor) {
                return Err(ToolError::PermissionDenied(format!(
                    "Path traversal blocked: parent directory '{}' escapes workspace jail '{}'",
                    canonical_ancestor.display(),
                    self.workspace_root.display()
                )));
            }

            // Reconstruct path and ensure no traversal
            let mut safe_path = canonical_ancestor;
            for part in remaining_parts.into_iter().rev() {
                safe_path.push(part);
            }

            Ok(safe_path)
        }
    }

    fn is_in_jail(&self, canonical_target: &Path) -> bool {
        if canonical_target.starts_with(&self.canonical_root) {
            return true;
        }
        for allowed in &self.allowed_external_paths {
            if canonical_target.starts_with(allowed) {
                return true;
            }
        }
        false
    }
}

// -------------------------------------------------------------
// Command Risk Classifier
// -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,
    Warning,
    Danger,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Safe => write!(f, "safe"),
            RiskLevel::Warning => write!(f, "warning"),
            RiskLevel::Danger => write!(f, "danger"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub reason: String,
}

/// Helper to tokenize a bash sub-command while respecting single and double quotes.
fn tokenize_bash_segment(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in segment.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                escaped = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Splits a compound bash command by operators (;, &&, ||, |, &, \n)
/// while ignoring operators enclosed within quotes.
fn split_bash_subcommands(command: &str) -> Vec<String> {
    let mut subcommands = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        if escaped {
            current.push(ch);
            escaped = false;
            i += 1;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                escaped = true;
                current.push(ch);
                i += 1;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
                i += 1;
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
                i += 1;
            }
            ';' | '\n' | '\r' if !in_single && !in_double => {
                if !current.trim().is_empty() {
                    subcommands.push(current.trim().to_string());
                }
                current = String::new();
                i += 1;
            }
            '&' if !in_single && !in_double => {
                if i + 1 < len && chars[i + 1] == '&' {
                    if !current.trim().is_empty() {
                        subcommands.push(current.trim().to_string());
                    }
                    current = String::new();
                    i += 2;
                } else {
                    if !current.trim().is_empty() {
                        subcommands.push(current.trim().to_string());
                    }
                    current = String::new();
                    i += 1;
                }
            }
            '|' if !in_single && !in_double => {
                if i + 1 < len && chars[i + 1] == '|' {
                    if !current.trim().is_empty() {
                        subcommands.push(current.trim().to_string());
                    }
                    current = String::new();
                    i += 2;
                } else {
                    if !current.trim().is_empty() {
                        subcommands.push(current.trim().to_string());
                    }
                    current = String::new();
                    i += 1;
                }
            }
            '`' if !in_single => {
                if !current.trim().is_empty() {
                    subcommands.push(current.trim().to_string());
                }
                current = String::new();
                i += 1;
            }
            '$' if !in_single && i + 1 < len && chars[i + 1] == '(' => {
                if !current.trim().is_empty() {
                    subcommands.push(current.trim().to_string());
                }
                current = String::new();
                i += 2;
            }
            ')' if !in_single && !in_double => {
                if !current.trim().is_empty() {
                    subcommands.push(current.trim().to_string());
                }
                current = String::new();
                i += 1;
            }
            _ => {
                current.push(ch);
                i += 1;
            }
        }
    }

    if !current.trim().is_empty() {
        subcommands.push(current.trim().to_string());
    }

    subcommands
}

/// Evaluates a bash command string and flags potentially dangerous operations.
pub fn assess_bash_command(command: &str) -> RiskAssessment {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return RiskAssessment {
            level: RiskLevel::Safe,
            reason: "Empty command".to_string(),
        };
    }

    let lower = trimmed.to_lowercase();

    // Never treat likely secret-bearing files or environment variables as ordinary
    // reads. This keeps `ask` mode human-in-the-loop for commands such as
    // `cat .env`, `printenv OPENAI_API_KEY`, and private-key inspection. Review
    // and trust modes remain explicit opt-outs chosen by the user.
    let sensitive_reference = lower.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '\'' | '"' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ';' | ',' | '$'
            )
        });
        token == ".env"
            || token.starts_with(".env.")
            || token.contains("credentials")
            || token.contains("secret")
            || token.contains("private.key")
            || token.contains("id_rsa")
            || token.contains("id_ed25519")
            || token.contains("api_key")
            || token.contains("access_token")
            || token.contains("access_key")
            || token == "printenv"
            || token == "env"
    });
    if sensitive_reference {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Access to a likely secret or environment variable".to_string(),
        };
    }

    let mut warning_candidate: Option<RiskAssessment> = None;

    // Check remote script pipe pattern across full pipeline
    if (lower.contains("curl ") || lower.contains("wget "))
        && (lower.contains("| sh") || lower.contains("| bash") || lower.contains("| zsh"))
    {
        warning_candidate = Some(RiskAssessment {
            level: RiskLevel::Warning,
            reason: "Piping remote internet script directly into shell".to_string(),
        });
    }

    let subcmds = split_bash_subcommands(trimmed);
    for subcmd in subcmds {
        let tokens = tokenize_bash_segment(&subcmd);
        if tokens.is_empty() {
            continue;
        }

        // 1. Skip environment variable assignments at the beginning of the command (e.g. FOO=bar)
        let mut idx = 0;
        while idx < tokens.len() {
            let tok = &tokens[idx];
            if tok.contains('=')
                && !tok.starts_with('-')
                && !tok.starts_with('/')
                && tok
                    .chars()
                    .next()
                    .map(|c| c.is_alphabetic() || c == '_')
                    .unwrap_or(false)
            {
                idx += 1;
            } else {
                break;
            }
        }

        if idx >= tokens.len() {
            continue;
        }

        // 2. Privilege escalation checks & wrapper skipping
        while idx < tokens.len() {
            let tok_lower = tokens[idx].to_lowercase();
            let tok_clean = tok_lower.trim_start_matches('\\');

            if tok_clean == "sudo" || tok_clean == "doas" {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Privilege escalation (sudo / doas)".to_string(),
                };
            }
            if tok_clean == "su" {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Switching user account (su)".to_string(),
                };
            }
            if ["builtin", "command", "exec", "nohup", "time", "env"].contains(&tok_clean) {
                idx += 1;
            } else {
                break;
            }
        }

        if idx >= tokens.len() {
            continue;
        }

        // 3. Inspect target executable
        let raw_bin = &tokens[idx];
        let bin_name = raw_bin
            .trim_start_matches('\\')
            .split('/')
            .next_back()
            .unwrap_or(raw_bin)
            .to_lowercase();
        let remaining_tokens = &tokens[idx + 1..];

        // Batch deletion via xargs
        if bin_name == "xargs"
            && remaining_tokens.iter().any(|t| {
                let name = t
                    .trim_start_matches('\\')
                    .split('/')
                    .next_back()
                    .unwrap_or(t)
                    .to_lowercase();
                ["rm", "rmdir", "unlink", "shred", "wipe", "srm"].contains(&name.as_str())
            })
        {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Batch deletion via xargs".to_string(),
            };
        }

        // Deletions: rm, rmdir, unlink, shred, wipe, srm
        if bin_name == "rm" {
            let is_recursive = remaining_tokens.iter().any(|t| {
                if t == "--recursive" {
                    return true;
                }
                if t.starts_with('-') && !t.starts_with("--") {
                    return t.contains('r') || t.contains('R');
                }
                false
            });

            if is_recursive {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Recursive directory or file deletion (rm -r / rm -rf)".to_string(),
                };
            } else {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "File or directory deletion (rm)".to_string(),
                };
            }
        }

        if bin_name == "rmdir" {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Directory deletion (rmdir)".to_string(),
            };
        }

        if bin_name == "unlink" {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "File deletion (unlink)".to_string(),
            };
        }

        if bin_name == "shred" {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "File shredding/deletion (shred)".to_string(),
            };
        }

        if bin_name == "wipe" || bin_name == "srm" {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Secure file deletion".to_string(),
            };
        }

        // Find deletions
        if bin_name == "find" {
            if remaining_tokens.iter().any(|t| t == "-delete") {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Batch file deletion (find -delete)".to_string(),
                };
            }
            if remaining_tokens
                .iter()
                .any(|t| t == "-exec" || t == "-execdir")
                && remaining_tokens.iter().any(|t| {
                    let name = t
                        .trim_start_matches('\\')
                        .split('/')
                        .next_back()
                        .unwrap_or(t)
                        .to_lowercase();
                    name == "rm"
                })
            {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Batch file deletion (find -exec rm)".to_string(),
                };
            }
        }

        // Git destructive operations
        if bin_name == "git" {
            let git_subcmd = remaining_tokens
                .iter()
                .find(|t| !t.starts_with('-'))
                .map(|s| s.to_lowercase());

            match git_subcmd.as_deref() {
                Some("rm") => {
                    return RiskAssessment {
                        level: RiskLevel::Danger,
                        reason: "Git file deletion (git rm)".to_string(),
                    };
                }
                Some("clean") => {
                    if remaining_tokens.iter().any(|t| {
                        t == "--force"
                            || (t.starts_with('-') && !t.starts_with("--") && t.contains('f'))
                    }) {
                        return RiskAssessment {
                            level: RiskLevel::Danger,
                            reason: "Destructive git operation that deletes uncommitted files (git clean)".to_string(),
                        };
                    }
                }
                Some("reset") => {
                    if remaining_tokens.iter().any(|t| t == "--hard") {
                        return RiskAssessment {
                            level: RiskLevel::Danger,
                            reason: "Destructive git reset that deletes uncommitted work (git reset --hard)".to_string(),
                        };
                    }
                }
                Some("push") => {
                    if remaining_tokens.iter().any(|t| {
                        t == "--force"
                            || (t.starts_with('-') && !t.starts_with("--") && t.contains('f'))
                            || t.starts_with('+')
                    }) {
                        return RiskAssessment {
                            level: RiskLevel::Danger,
                            reason: "Force pushing git history".to_string(),
                        };
                    }
                }
                Some("branch") => {
                    if remaining_tokens.iter().any(|t| t == "-D") {
                        return RiskAssessment {
                            level: RiskLevel::Danger,
                            reason: "Force deleting git branch (git branch -D)".to_string(),
                        };
                    }
                    if remaining_tokens
                        .iter()
                        .any(|t| t == "-d" || t == "--delete")
                        && warning_candidate.is_none()
                    {
                        warning_candidate = Some(RiskAssessment {
                            level: RiskLevel::Warning,
                            reason: "Deleting git branch".to_string(),
                        });
                    }
                }
                Some("stash")
                    if remaining_tokens.iter().any(|t| t == "drop" || t == "clear")
                        && warning_candidate.is_none() =>
                {
                    warning_candidate = Some(RiskAssessment {
                        level: RiskLevel::Warning,
                        reason: "Discarding git stash".to_string(),
                    });
                }
                _ => {}
            }
        }

        // Direct disk / partition operations
        if bin_name == "mkfs"
            || bin_name.starts_with("mkfs.")
            || bin_name == "fdisk"
            || bin_name == "parted"
            || bin_name == "gdisk"
        {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Direct disk formatting / partition modification".to_string(),
            };
        }

        if bin_name == "dd"
            && remaining_tokens
                .iter()
                .any(|t| t.starts_with("if=") || t.starts_with("of="))
        {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Direct disk write / raw image manipulation (dd)".to_string(),
            };
        }

        // System shutdown / reboot
        if bin_name == "reboot"
            || bin_name == "shutdown"
            || bin_name == "poweroff"
            || bin_name == "init"
        {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "System shutdown / reboot command".to_string(),
            };
        }

        // Process termination
        if bin_name == "pkill" || bin_name == "killall" {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Process termination command".to_string(),
            };
        }
        if bin_name == "kill"
            && remaining_tokens
                .iter()
                .any(|t| t == "-9" || t == "-KILL" || t == "-kill" || t == "-s 9")
        {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Force process termination (kill -9)".to_string(),
            };
        }

        // Permissions & Firewall
        if bin_name == "chmod" {
            let has_recursive = remaining_tokens.iter().any(|t| {
                (t.starts_with('-') && !t.starts_with("--") && (t.contains('R') || t.contains('r')))
                    || t == "--recursive"
            });
            let has_permissive = remaining_tokens.iter().any(|t| t == "777" || t == "a+rwx");
            if has_recursive && has_permissive {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Broad recursive permission alteration (chmod -R 777)".to_string(),
                };
            }
        }
        if bin_name == "chown" {
            let has_recursive = remaining_tokens.iter().any(|t| {
                (t.starts_with('-') && !t.starts_with("--") && (t.contains('R') || t.contains('r')))
                    || t == "--recursive"
            });
            if has_recursive {
                return RiskAssessment {
                    level: RiskLevel::Danger,
                    reason: "Broad recursive ownership alteration (chown -R)".to_string(),
                };
            }
        }

        if bin_name == "iptables"
            || bin_name == "ufw"
            || bin_name == "nft"
            || bin_name == "firewalld"
        {
            return RiskAssessment {
                level: RiskLevel::Danger,
                reason: "Firewall / network routing modification".to_string(),
            };
        }

        // Registry publish warnings
        if (["cargo", "npm", "pnpm", "yarn"].contains(&bin_name.as_str()))
            && remaining_tokens.iter().any(|t| t == "publish")
            && warning_candidate.is_none()
        {
            warning_candidate = Some(RiskAssessment {
                level: RiskLevel::Warning,
                reason: "Publishing package to public registry".to_string(),
            });
        }
    }

    if let Some(w) = warning_candidate {
        return w;
    }

    RiskAssessment {
        level: RiskLevel::Safe,
        reason: "Standard development command".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_assessment_danger_deletions() {
        // Recursive deletions
        assert_eq!(
            assess_bash_command("rm -rf target/").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("rm -r src/old").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("rm --recursive temp/").level,
            RiskLevel::Danger
        );

        // Single file / non-recursive deletions
        assert_eq!(assess_bash_command("rm file.txt").level, RiskLevel::Danger);
        assert_eq!(
            assess_bash_command("rm -f file.txt").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("/bin/rm foo.txt").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("rmdir empty_dir").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("unlink test.txt").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("shred -u secret.txt").level,
            RiskLevel::Danger
        );

        // Chained deletions
        assert_eq!(
            assess_bash_command("echo 'done' && rm test.txt").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("rm -rf ~/Work/rekarta-theme && echo 'deleted' && ls").level,
            RiskLevel::Danger
        );

        // Batch deletions
        assert_eq!(
            assess_bash_command("cat files.txt | xargs rm").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("find . -name '*.tmp' -delete").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("find . -name '*.tmp' -exec rm {} +").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("git rm src/old.rs").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("git clean -fd").level,
            RiskLevel::Danger
        );
    }

    #[test]
    fn test_risk_assessment_danger_system() {
        assert_eq!(
            assess_bash_command("sudo apt update").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("git reset --hard HEAD~1").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("git push --force origin main").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("pkill -9 maukode").level,
            RiskLevel::Danger
        );
        assert_eq!(assess_bash_command("kill -9 1234").level, RiskLevel::Danger);
        assert_eq!(
            assess_bash_command("mkfs.ext4 /dev/sdb1").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("dd if=/dev/zero of=/dev/sda").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("chmod -R 777 /var/www").level,
            RiskLevel::Danger
        );
    }

    #[test]
    fn test_risk_assessment_warning() {
        assert_eq!(
            assess_bash_command("curl -fsSL https://get.example.com | sh").level,
            RiskLevel::Warning
        );
        assert_eq!(
            assess_bash_command("git branch -d old-feat").level,
            RiskLevel::Warning
        );
        assert_eq!(
            assess_bash_command("git stash drop").level,
            RiskLevel::Warning
        );
        assert_eq!(
            assess_bash_command("cargo publish").level,
            RiskLevel::Warning
        );
    }

    #[test]
    fn test_risk_assessment_sensitive_environment_access() {
        for command in [
            "cat .env",
            "cat .env.production",
            "printenv OPENAI_API_KEY",
            "echo $AWS_ACCESS_KEY_ID",
            "ssh-add ~/.ssh/id_ed25519",
        ] {
            assert_eq!(
                assess_bash_command(command).level,
                RiskLevel::Danger,
                "expected sensitive command to require approval: {command}"
            );
        }
    }
    #[test]
    fn test_risk_assessment_safe() {
        assert_eq!(
            assess_bash_command("cargo test --workspace").level,
            RiskLevel::Safe
        );
        assert_eq!(assess_bash_command("git status").level, RiskLevel::Safe);
        assert_eq!(
            assess_bash_command("cat src/main.rs").level,
            RiskLevel::Safe
        );
        assert_eq!(
            assess_bash_command("cargo run --example form").level,
            RiskLevel::Safe
        );
        assert_eq!(
            assess_bash_command("grep 'rm -rf' src/security.rs").level,
            RiskLevel::Safe
        );
        assert_eq!(
            assess_bash_command("echo 'rm -rf is dangerous'").level,
            RiskLevel::Safe
        );
        assert_eq!(
            assess_bash_command("ls -la /home/user/Work").level,
            RiskLevel::Safe
        );
    }

    #[test]
    fn test_jail_path_traversal() {
        let temp_dir = std::env::temp_dir().join("maukode_jail_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let jail = PathJail::new(&temp_dir);

        // Child file inside jail
        let inside = temp_dir.join("hello.txt");
        let _ = std::fs::write(&inside, "content");
        assert!(jail.validate_path("hello.txt").is_ok());

        // Escape attempts
        assert!(jail.validate_path("/etc/passwd").is_err());
        assert!(jail.validate_path("../../etc/shadow").is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
