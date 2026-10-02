use std::path::{Component, Path, PathBuf};
use crate::tool::ToolError;

#[derive(Debug, Clone)]
pub struct PathJail {
    pub workspace_root: PathBuf,
    pub canonical_root: PathBuf,
    pub allowed_external_paths: Vec<PathBuf>,
}

impl PathJail {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let root = workspace_root.into();
        let canonical_root = root.canonicalize().unwrap_or_else(|_| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        });
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
                ancestor
                    .canonicalize()
                    .map_err(|e| ToolError::Execution(format!("Cannot resolve ancestor path: {e}")))?
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

/// Evaluates a bash command string and flags potentially dangerous operations.
pub fn assess_bash_command(command: &str) -> RiskAssessment {
    let trimmed = command.trim();
    let lower = trimmed.to_lowercase();

    // 1. Extreme danger: Destructive deletions, privilege escalation, force resets
    if lower.contains("rm ")
        && (lower.contains("-rf")
            || lower.contains("-fr")
            || lower.contains("-r ")
            || lower.ends_with("-r"))
    {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Recursive directory or file deletion (rm -r / rm -rf)".to_string(),
        };
    }
    if lower.starts_with("sudo ") || lower.contains(" sudo ") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Privilege escalation (sudo)".to_string(),
        };
    }
    if lower.starts_with("su ") || lower.contains(" su ") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Switching user account (su)".to_string(),
        };
    }
    if lower.contains("chmod -r 777") || lower.contains("chown -r") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Broad recursive permission alteration".to_string(),
        };
    }
    if lower.contains("git reset --hard") || lower.contains("git clean -fd") || lower.contains("git clean -f") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Destructive git operation that deletes uncommitted work".to_string(),
        };
    }
    if lower.contains("git push") && (lower.contains("--force") || lower.contains("-f")) {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Force pushing git history".to_string(),
        };
    }
    if lower.starts_with("mkfs")
        || lower.contains(" dd if=")
        || lower.starts_with("dd if=")
        || lower.starts_with("fdisk")
    {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Direct disk formatting / partition modification".to_string(),
        };
    }
    if lower.starts_with("reboot") || lower.starts_with("shutdown") || lower.starts_with("poweroff") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "System shutdown / reboot command".to_string(),
        };
    }
    if lower.starts_with("pkill") || lower.starts_with("killall") || lower.contains("kill -9") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Process termination command".to_string(),
        };
    }
    if lower.starts_with("iptables") || lower.starts_with("ufw") {
        return RiskAssessment {
            level: RiskLevel::Danger,
            reason: "Firewall / network routing modification".to_string(),
        };
    }

    // 2. Warning level: Piping untrusted internet scripts, branch deletion, stash drop
    if (lower.contains("curl ") || lower.contains("wget "))
        && (lower.contains("| sh") || lower.contains("| bash"))
    {
        return RiskAssessment {
            level: RiskLevel::Warning,
            reason: "Piping remote internet script directly into shell".to_string(),
        };
    }
    if lower.contains("git branch -d") || lower.contains("git stash drop") {
        return RiskAssessment {
            level: RiskLevel::Warning,
            reason: "Deleting git branch or discarding git stash".to_string(),
        };
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
    fn test_risk_assessment_danger() {
        assert_eq!(
            assess_bash_command("rm -rf target/").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("sudo apt update").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("git reset --hard HEAD~1").level,
            RiskLevel::Danger
        );
        assert_eq!(
            assess_bash_command("pkill -9 maukode").level,
            RiskLevel::Danger
        );
    }

    #[test]
    fn test_risk_assessment_warning() {
        assert_eq!(
            assess_bash_command("curl -fsSL https://get.example.com | sh").level,
            RiskLevel::Warning
        );
    }

    #[test]
    fn test_risk_assessment_safe() {
        assert_eq!(
            assess_bash_command("cargo test --workspace").level,
            RiskLevel::Safe
        );
        assert_eq!(
            assess_bash_command("git status").level,
            RiskLevel::Safe
        );
        assert_eq!(
            assess_bash_command("cat src/main.rs").level,
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
