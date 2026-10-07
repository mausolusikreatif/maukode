use maukode_protocol::SkillDefinition;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Manages built-in, user-configured, and workspace-local skills.
#[derive(Debug, Clone)]
pub struct SkillManager {
    built_in_skills: HashMap<String, SkillDefinition>,
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillManager {
    pub fn new() -> Self {
        let mut built_ins = HashMap::new();
        for skill in SkillDefinition::built_in_skills() {
            built_ins.insert(skill.name.clone(), skill);
        }
        Self {
            built_in_skills: built_ins,
        }
    }

    pub fn with_built_in(mut self, skill: SkillDefinition) -> Self {
        self.built_in_skills.insert(skill.name.clone(), skill);
        self
    }

    /// Parse skill markdown content, extracting YAML frontmatter if present.
    pub fn parse_skill_content(
        content: &str,
        default_name: &str,
        path: Option<&Path>,
    ) -> SkillDefinition {
        let lines: Vec<&str> = content.lines().collect();
        let mut frontmatter_lines: Vec<&str> = Vec::new();
        let mut body_lines: Vec<&str> = Vec::new();
        let mut found_frontmatter = false;

        if let Some(first_idx) = lines.iter().position(|l| !l.trim().is_empty()) {
            if lines[first_idx].trim() == "---" {
                let mut idx = first_idx + 1;
                while idx < lines.len() {
                    let line = lines[idx];
                    if line.trim() == "---" {
                        found_frontmatter = true;
                        idx += 1;
                        body_lines.extend_from_slice(&lines[idx..]);
                        break;
                    } else {
                        frontmatter_lines.push(line);
                        idx += 1;
                    }
                }
            }
        }

        if !found_frontmatter {
            body_lines = lines;
        }

        let mut parsed_name: Option<String> = None;
        let mut parsed_description: Option<String> = None;
        let mut tools: Vec<String> = Vec::new();
        let mut run_as_subagent = false;
        let mut current_key: Option<String> = None;

        for line in frontmatter_lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("- ") {
                if current_key.as_deref() == Some("tools") {
                    let tool_name = rest.trim().trim_matches('"').trim_matches('\'');
                    if !tool_name.is_empty() {
                        tools.push(tool_name.to_string());
                    }
                }
                continue;
            }

            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim();
                current_key = Some(key.clone());

                match key.as_str() {
                    "name" => {
                        let n = val.trim_matches('"').trim_matches('\'');
                        if !n.is_empty() {
                            parsed_name = Some(n.to_string());
                        }
                    }
                    "description" => {
                        let d = val.trim_matches('"').trim_matches('\'');
                        if !d.is_empty() {
                            parsed_description = Some(d.to_string());
                        }
                    }
                    "run_as_subagent" => {
                        let val_lower = val.to_lowercase();
                        run_as_subagent =
                            val_lower == "true" || val_lower == "1" || val_lower == "yes";
                    }
                    "tools" => {
                        let val_trimmed = val.trim();
                        if val_trimmed.starts_with('[') && val_trimmed.ends_with(']') {
                            let inner = &val_trimmed[1..val_trimmed.len() - 1];
                            for item in inner.split(',') {
                                let t = item.trim().trim_matches('"').trim_matches('\'');
                                if !t.is_empty() {
                                    tools.push(t.to_string());
                                }
                            }
                        } else if !val_trimmed.is_empty() {
                            for item in val_trimmed.split(',') {
                                let t = item.trim().trim_matches('"').trim_matches('\'');
                                if !t.is_empty() {
                                    tools.push(t.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let name = parsed_name.unwrap_or_else(|| default_name.to_string());
        let description = parsed_description.unwrap_or_else(|| {
            for line in &body_lines {
                let t = line.trim();
                if !t.is_empty() {
                    let stripped = t.trim_start_matches('#').trim();
                    if !stripped.is_empty() && stripped.len() <= 120 {
                        return stripped.to_string();
                    }
                    break;
                }
            }
            format!("Skill {name}")
        });

        let prompt = body_lines.join("\n").trim().to_string();
        let allowed_tools = if tools.is_empty() { None } else { Some(tools) };
        let path_str = path.map(|p| p.to_string_lossy().to_string());

        SkillDefinition {
            name,
            description,
            prompt,
            allowed_tools,
            path: path_str,
            is_built_in: false,
            run_as_subagent,
        }
    }

    /// Load all skills from a specific directory.
    pub fn load_skills_from_dir(&self, dir: &Path) -> Vec<SkillDefinition> {
        let mut skills = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return skills;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if path.extension().and_then(|s| s.to_str()) == Some("md") {
                    let default_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("skill");
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        skills.push(Self::parse_skill_content(
                            &content,
                            default_name,
                            Some(&path),
                        ));
                    }
                }
            } else if path.is_dir() {
                // Check for SKILL.md or skill.md inside subdirectory
                let skill_md = path.join("SKILL.md");
                let skill_md_lower = path.join("skill.md");
                let target = if skill_md.is_file() {
                    Some(skill_md)
                } else if skill_md_lower.is_file() {
                    Some(skill_md_lower)
                } else {
                    None
                };

                if let Some(target_file) = target {
                    let default_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("skill");
                    if let Ok(content) = std::fs::read_to_string(&target_file) {
                        skills.push(Self::parse_skill_content(
                            &content,
                            default_name,
                            Some(&target_file),
                        ));
                    }
                }
            }
        }

        skills
    }

    pub fn user_skills_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(config_dir) = crate::paths::config_dir() {
            dirs.push(config_dir.join("maukode").join("skills"));
        }
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".maukode").join("skills"));
        }
        dirs
    }

    pub fn workspace_skills_dirs(&self, workspace_dir: &Path) -> Vec<PathBuf> {
        vec![
            workspace_dir.join(".maukode").join("skills"),
            workspace_dir.join(".skills"),
        ]
    }

    /// List all available skills merged with standard precedence:
    /// Workspace skills > User skills > Built-in skills.
    pub fn list_skills(&self, workspace_dir: Option<&Path>) -> Vec<SkillDefinition> {
        let mut map: HashMap<String, SkillDefinition> = HashMap::new();

        // 1. Built-in skills (lowest precedence)
        for skill in self.built_in_skills.values() {
            map.insert(skill.name.clone(), skill.clone());
        }

        // 2. User skills (overrides built-in)
        for dir in self.user_skills_dirs() {
            for skill in self.load_skills_from_dir(&dir) {
                map.insert(skill.name.clone(), skill);
            }
        }

        // 3. Workspace skills (highest precedence)
        if let Some(ws) = workspace_dir {
            for dir in self.workspace_skills_dirs(ws) {
                for skill in self.load_skills_from_dir(&dir) {
                    map.insert(skill.name.clone(), skill);
                }
            }
        }

        let mut result: Vec<SkillDefinition> = map.into_values().collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Get a single skill by name respecting workspace > user > built-in precedence.
    pub fn get_skill(&self, name: &str, workspace_dir: Option<&Path>) -> Option<SkillDefinition> {
        // 1. Workspace skills first
        if let Some(ws) = workspace_dir {
            for dir in self.workspace_skills_dirs(ws) {
                for skill in self.load_skills_from_dir(&dir) {
                    if skill.name == name {
                        return Some(skill);
                    }
                }
            }
        }

        // 2. User skills next
        for dir in self.user_skills_dirs() {
            for skill in self.load_skills_from_dir(&dir) {
                if skill.name == name {
                    return Some(skill);
                }
            }
        }

        // 3. Built-in skills fallback
        self.built_in_skills.get(name).cloned()
    }

    /// Expand a skill prompt by injecting user arguments or appending a Task section.
    pub fn expand_skill_prompt(
        &self,
        name: &str,
        user_args: &str,
        workspace_dir: Option<&Path>,
    ) -> Option<String> {
        let skill = self.get_skill(name, workspace_dir)?;
        let prompt = skill.prompt;
        let trimmed_args = user_args.trim();
        if trimmed_args.is_empty() {
            return Some(prompt);
        }

        if prompt.contains("$ARGUMENTS") {
            Some(prompt.replace("$ARGUMENTS", trimmed_args))
        } else if prompt.contains("{{arguments}}") {
            Some(prompt.replace("{{arguments}}", trimmed_args))
        } else if prompt.contains("{{args}}") {
            Some(prompt.replace("{{args}}", trimmed_args))
        } else {
            Some(format!("{}\n\nTask:\n{}", prompt.trim(), trimmed_args))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_skill_with_frontmatter() {
        let content = r#"---
name: my-custom-skill
description: Performs custom analysis
tools: [read_file, grep]
run_as_subagent: true
---
# Instructions
Do the analysis thoroughly.
"#;
        let skill = SkillManager::parse_skill_content(content, "fallback", None);
        assert_eq!(skill.name, "my-custom-skill");
        assert_eq!(skill.description, "Performs custom analysis");
        assert_eq!(
            skill.allowed_tools,
            Some(vec!["read_file".into(), "grep".into()])
        );
        assert!(skill.run_as_subagent);
        assert_eq!(skill.prompt, "# Instructions\nDo the analysis thoroughly.");
        assert!(!skill.is_built_in);
    }

    #[test]
    fn test_parse_skill_list_tools() {
        let content = r#"---
name: list-tool-skill
description: Skill with list tools
tools:
  - read_file
  - glob
  - grep
---
Prompt body
"#;
        let skill = SkillManager::parse_skill_content(content, "fallback", None);
        assert_eq!(skill.name, "list-tool-skill");
        assert_eq!(
            skill.allowed_tools,
            Some(vec!["read_file".into(), "glob".into(), "grep".into()])
        );
        assert_eq!(skill.prompt, "Prompt body");
    }

    #[test]
    fn test_parse_skill_without_frontmatter() {
        let content = "# Code Explainer\nExplain the selected codebase carefully.";
        let skill = SkillManager::parse_skill_content(content, "code-explainer", None);
        assert_eq!(skill.name, "code-explainer");
        assert_eq!(skill.description, "Code Explainer");
        assert_eq!(skill.allowed_tools, None);
        assert!(!skill.run_as_subagent);
        assert_eq!(skill.prompt, content);
    }

    #[test]
    fn test_expand_skill_prompt() {
        let manager = SkillManager::new();
        let expanded =
            manager.expand_skill_prompt("review-pr", "PR #42 in crates/maukode-core", None);
        assert!(expanded.is_some());
        let exp = expanded.unwrap();
        assert!(exp.contains("PR #42 in crates/maukode-core"));
    }

    #[test]
    fn test_expand_skill_prompt_with_variables() {
        let content = "Review $ARGUMENTS thoroughly.";
        let skill = SkillManager::parse_skill_content(content, "templated", None);
        let manager = SkillManager::new().with_built_in(skill);

        let expanded = manager
            .expand_skill_prompt("templated", "commit abc123", None)
            .unwrap();
        assert_eq!(expanded, "Review commit abc123 thoroughly.");
    }

    #[test]
    fn test_directory_discovery_and_overrides() {
        let temp = tempdir().unwrap();
        let ws_skills_dir = temp.path().join(".maukode").join("skills");
        std::fs::create_dir_all(&ws_skills_dir).unwrap();

        // Override review-pr in workspace
        let custom_review_pr = r#"---
name: review-pr
description: Workspace-specific PR reviewer
---
Custom workspace PR review prompt.
"#;
        std::fs::write(ws_skills_dir.join("review-pr.md"), custom_review_pr).unwrap();

        // Add a brand new skill inside folder with SKILL.md
        let helper_dir = ws_skills_dir.join("code-helper");
        std::fs::create_dir_all(&helper_dir).unwrap();
        let helper_skill = r#"---
name: code-helper
description: Helpful code assistant
---
Code helper prompt.
"#;
        std::fs::write(helper_dir.join("SKILL.md"), helper_skill).unwrap();

        let manager = SkillManager::new();
        let skills = manager.list_skills(Some(temp.path()));

        // review-pr should be overridden
        let review = skills.iter().find(|s| s.name == "review-pr").unwrap();
        assert_eq!(review.description, "Workspace-specific PR reviewer");
        assert_eq!(review.prompt, "Custom workspace PR review prompt.");

        // code-helper should be discovered
        let helper = skills.iter().find(|s| s.name == "code-helper").unwrap();
        assert_eq!(helper.description, "Helpful code assistant");
        assert_eq!(helper.prompt, "Code helper prompt.");

        // fix-ci built-in should still exist
        assert!(skills.iter().any(|s| s.name == "fix-ci"));
    }
}
