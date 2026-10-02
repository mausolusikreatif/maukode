pub mod init;

pub use init::{init_project, DetectedStack, InitResult, ProjectDetector};

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSource {
    GlobalUser(PathBuf),
    ProjectAgents(PathBuf),
    ProjectMauKode(PathBuf),
    ProjectClaude(PathBuf),
    ProjectDotRc(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRule {
    pub source: RuleSource,
    pub path: PathBuf,
    pub content: String,
}

pub struct ProjectMemory {
    cwd: PathBuf,
}

impl ProjectMemory {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Check global user rules in ~/.config/maukode/ or ~/.maukode/
    pub async fn discover_global_rules(&self) -> Option<DiscoveredRule> {
        let mut candidates = Vec::new();
        if let Some(config_dir) = dirs::config_dir() {
            candidates.push(config_dir.join("maukode").join("RULES.md"));
            candidates.push(config_dir.join("maukode").join("AGENTS.md"));
        }
        if let Some(home_dir) = dirs::home_dir() {
            candidates.push(home_dir.join(".config").join("maukode").join("RULES.md"));
            candidates.push(home_dir.join(".config").join("maukode").join("AGENTS.md"));
            candidates.push(home_dir.join(".maukode").join("RULES.md"));
            candidates.push(home_dir.join(".maukode").join("AGENTS.md"));
        }

        for path in candidates {
            if path.is_file() {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        return Some(DiscoveredRule {
                            source: RuleSource::GlobalUser(path.clone()),
                            path,
                            content: trimmed.to_string(),
                        });
                    }
                }
            }
        }
        None
    }

    /// Discover project instruction files hierarchically from cwd upwards
    pub async fn discover_project_rules(&self) -> Vec<DiscoveredRule> {
        let mut discovered = Vec::new();
        let mut curr = self.cwd.clone();

        loop {
            // Check candidate files in priority order for this directory level:
            // 1. AGENTS.md (Standard)
            // 2. .maukode/rules.md
            // 3. MAUKODE.md
            // 4. CLAUDE.md
            // 5. .maukoderc
            let candidates = [
                "AGENTS.md",
                ".maukode/rules.md",
                "MAUKODE.md",
                "CLAUDE.md",
                ".maukoderc",
            ];

            for rel_path in candidates {
                let candidate_path = curr.join(rel_path);
                if candidate_path.is_file() {
                    if let Ok(content) = tokio::fs::read_to_string(&candidate_path).await {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            let source = match rel_path {
                                "AGENTS.md" => RuleSource::ProjectAgents(candidate_path.clone()),
                                "CLAUDE.md" => RuleSource::ProjectClaude(candidate_path.clone()),
                                ".maukoderc" => RuleSource::ProjectDotRc(candidate_path.clone()),
                                _ => RuleSource::ProjectMauKode(candidate_path.clone()),
                            };
                            discovered.push(DiscoveredRule {
                                source,
                                path: candidate_path,
                                content: trimmed.to_string(),
                            });
                            // Take highest priority rule in this directory
                            break;
                        }
                    }
                }
            }

            if !curr.pop() {
                break;
            }
        }

        // Reverse so that root/ancestor rules appear first, followed by child rules
        discovered.reverse();
        discovered
    }

    /// Discover all active rules (global user + project hierarchy)
    pub async fn discover_all(&self) -> Vec<DiscoveredRule> {
        let mut all = Vec::new();
        if let Some(global) = self.discover_global_rules().await {
            all.push(global);
        }
        let project_rules = self.discover_project_rules().await;
        all.extend(project_rules);
        all
    }

    /// Render merged instructions ready for injection into system prompt
    pub async fn render_merged_rules(&self) -> String {
        let mut sections = Vec::new();

        if let Some(global) = self.discover_global_rules().await {
            sections.push(format!(
                "--- Global User Guidelines ({}) ---\n{}\n-----------------------------------",
                global.path.display(),
                global.content
            ));
        }

        let project_rules = self.discover_project_rules().await;
        for rule in project_rules {
            let filename = rule.path.file_name().and_then(|n| n.to_str()).unwrap_or("rules");
            sections.push(format!(
                "--- Project Instructions ({filename} from {}) ---\n{}\n-----------------------------------",
                rule.path.display(),
                rule.content
            ));
        }

        sections.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_project_memory_hierarchical_discovery() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("packages").join("app");
        tokio::fs::create_dir_all(&sub).await.unwrap();

        // Root has CLAUDE.md
        tokio::fs::write(root.join("CLAUDE.md"), "# Root Guidelines\nAlways write tests.").await.unwrap();

        // Sub directory has standard AGENTS.md
        tokio::fs::write(sub.join("AGENTS.md"), "# App Guidelines\nUse TypeScript strict mode.").await.unwrap();

        let memory = ProjectMemory::new(&sub);
        let rules = memory.discover_project_rules().await;

        assert_eq!(rules.len(), 2);
        // Ancestor rule first
        assert!(rules[0].content.contains("Root Guidelines"));
        // Child rule second
        assert!(rules[1].content.contains("App Guidelines"));

        let merged = memory.render_merged_rules().await;
        assert!(merged.contains("Root Guidelines"));
        assert!(merged.contains("App Guidelines"));
    }

    #[tokio::test]
    async fn test_init_project_detection_and_force() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Mock a Rust package
        tokio::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"sample-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ).await.unwrap();

        let res = init_project(root, false).await.unwrap();
        assert!(res.created);
        assert!(!res.overwritten);
        assert_eq!(res.path, root.join("AGENTS.md"));
        assert!(res.content.contains("# Project: sample-crate"));
        assert!(res.content.contains("cargo test"));

        // Running without force should report already exists
        let res_repeat = init_project(root, false).await.unwrap();
        assert!(!res_repeat.created);
        assert!(!res_repeat.overwritten);
        assert!(res_repeat.message.contains("already exists"));

        // Running with force should overwrite
        let res_force = init_project(root, true).await.unwrap();
        assert!(res_force.overwritten);
        assert!(res_force.message.contains("Overwrote"));
    }
}
