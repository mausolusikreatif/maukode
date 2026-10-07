use crate::config::McpServerConfig;
use crate::skills::SkillManager;
use maukode_protocol::{PluginManifest, SkillDefinition};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Manages discovery, loading, and scoping of MauKode plugins.
#[derive(Debug, Clone, Default)]
pub struct PluginManager {
    disabled_plugins: HashMap<String, bool>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            disabled_plugins: HashMap::new(),
        }
    }

    /// Set enabled status for a plugin by name.
    pub fn set_plugin_enabled(&mut self, name: &str, enabled: bool) {
        if enabled {
            self.disabled_plugins.remove(name);
        } else {
            self.disabled_plugins.insert(name.to_string(), true);
        }
    }

    /// Check if a plugin is enabled.
    pub fn is_plugin_enabled(&self, manifest: &PluginManifest) -> bool {
        if self
            .disabled_plugins
            .get(&manifest.name)
            .copied()
            .unwrap_or(false)
        {
            return false;
        }
        manifest.is_enabled
    }

    /// Load a plugin from a plugin directory containing `plugin.json` or `maukode-plugin.json`.
    pub fn load_plugin_from_dir(&self, dir: &Path) -> Option<PluginManifest> {
        let manifest_file = if dir.join("plugin.json").is_file() {
            dir.join("plugin.json")
        } else if dir.join("maukode-plugin.json").is_file() {
            dir.join("maukode-plugin.json")
        } else {
            return None;
        };

        let content = std::fs::read_to_string(&manifest_file).ok()?;
        let mut manifest: PluginManifest = serde_json::from_str(&content).ok()?;
        manifest.path = Some(dir.to_string_lossy().to_string());

        // Respect in-memory disabled overrides
        if self
            .disabled_plugins
            .get(&manifest.name)
            .copied()
            .unwrap_or(false)
        {
            manifest.is_enabled = false;
        }

        // 1. Discover bundled skills from <plugin_dir>/skills/
        let skills_dir = dir.join("skills");
        if skills_dir.is_dir() {
            let skill_manager = SkillManager::new();
            let discovered_skills = skill_manager.load_skills_from_dir(&skills_dir);
            for s in discovered_skills {
                if !manifest
                    .skills
                    .iter()
                    .any(|existing| existing.name == s.name)
                {
                    manifest.skills.push(s);
                }
            }
        }

        // 2. Discover bundled rules from <plugin_dir>/rules.md or <plugin_dir>/rules/
        let rules_file = dir.join("rules.md");
        if rules_file.is_file() {
            if let Ok(rule_content) = std::fs::read_to_string(&rules_file) {
                let trimmed = rule_content.trim();
                if !trimmed.is_empty() && !manifest.rules.iter().any(|r| r == trimmed) {
                    manifest.rules.push(trimmed.to_string());
                }
            }
        }

        let rules_dir = dir.join("rules");
        if rules_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&rules_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                        if let Ok(rule_content) = std::fs::read_to_string(&path) {
                            let trimmed = rule_content.trim();
                            if !trimmed.is_empty() && !manifest.rules.iter().any(|r| r == trimmed) {
                                manifest.rules.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }

        Some(manifest)
    }

    /// Load all plugins in a given parent directory.
    pub fn load_plugins_from_parent_dir(&self, dir: &Path) -> Vec<PluginManifest> {
        let mut plugins = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return plugins;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(plugin) = self.load_plugin_from_dir(&path) {
                    plugins.push(plugin);
                }
            }
        }

        plugins
    }

    pub fn user_plugins_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(config_dir) = crate::paths::config_dir() {
            dirs.push(config_dir.join("maukode").join("plugins"));
        }
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".maukode").join("plugins"));
        }
        dirs
    }

    pub fn workspace_plugins_dirs(&self, workspace_dir: &Path) -> Vec<PathBuf> {
        vec![
            workspace_dir.join(".maukode").join("plugins"),
            workspace_dir.join(".plugins"),
        ]
    }

    /// List all discovered plugins with workspace overriding user plugins.
    pub fn list_plugins(&self, workspace_dir: Option<&Path>) -> Vec<PluginManifest> {
        let mut map: HashMap<String, PluginManifest> = HashMap::new();

        // 1. User plugins (lower precedence)
        for dir in self.user_plugins_dirs() {
            for plugin in self.load_plugins_from_parent_dir(&dir) {
                map.insert(plugin.name.clone(), plugin);
            }
        }

        // 2. Workspace plugins (higher precedence)
        if let Some(ws) = workspace_dir {
            for dir in self.workspace_plugins_dirs(ws) {
                for plugin in self.load_plugins_from_parent_dir(&dir) {
                    map.insert(plugin.name.clone(), plugin);
                }
            }
        }

        let mut list: Vec<PluginManifest> = map.into_values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    /// Get a single plugin by name.
    pub fn get_plugin(&self, name: &str, workspace_dir: Option<&Path>) -> Option<PluginManifest> {
        if let Some(ws) = workspace_dir {
            for dir in self.workspace_plugins_dirs(ws) {
                for plugin in self.load_plugins_from_parent_dir(&dir) {
                    if plugin.name == name {
                        return Some(plugin);
                    }
                }
            }
        }

        for dir in self.user_plugins_dirs() {
            for plugin in self.load_plugins_from_parent_dir(&dir) {
                if plugin.name == name {
                    return Some(plugin);
                }
            }
        }

        None
    }

    /// Collect all skills bundled into currently enabled plugins.
    pub fn enabled_skills(&self, workspace_dir: Option<&Path>) -> Vec<SkillDefinition> {
        let mut skills = Vec::new();
        for plugin in self.list_plugins(workspace_dir) {
            if self.is_plugin_enabled(&plugin) {
                skills.extend(plugin.skills);
            }
        }
        skills
    }

    /// Collect all MCP server configurations from currently enabled plugins.
    pub fn enabled_mcp_servers(
        &self,
        workspace_dir: Option<&Path>,
    ) -> HashMap<String, McpServerConfig> {
        let mut servers = HashMap::new();
        for plugin in self.list_plugins(workspace_dir) {
            if self.is_plugin_enabled(&plugin) {
                for (name, cfg) in plugin.mcp_servers {
                    servers.insert(
                        name,
                        McpServerConfig {
                            command: cfg.command,
                            args: cfg.args,
                            env: cfg.env,
                        },
                    );
                }
            }
        }
        servers
    }

    /// Collect all rules from currently enabled plugins.
    pub fn enabled_rules(&self, workspace_dir: Option<&Path>) -> Vec<String> {
        let mut rules = Vec::new();
        for plugin in self.list_plugins(workspace_dir) {
            if self.is_plugin_enabled(&plugin) {
                rules.extend(plugin.rules);
            }
        }
        rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_plugin_manifest_with_skills_and_mcp() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("test-plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let manifest_json = r#"{
            "name": "test-plugin",
            "version": "1.0.0",
            "description": "A test plugin",
            "author": "maukode",
            "is_enabled": true,
            "skills": [
                {
                    "name": "plugin-skill",
                    "description": "Skill from plugin",
                    "prompt": "Do plugin things",
                    "is_built_in": false,
                    "run_as_subagent": false
                }
            ],
            "mcp_servers": {
                "test-mcp": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {}
                }
            },
            "rules": ["Follow standard rules"]
        }"#;

        std::fs::write(plugin_dir.join("plugin.json"), manifest_json).unwrap();

        let manager = PluginManager::new();
        let plugin = manager.load_plugin_from_dir(&plugin_dir).unwrap();

        assert_eq!(plugin.name, "test-plugin");
        assert_eq!(plugin.version, "1.0.0");
        assert_eq!(plugin.skills.len(), 1);
        assert_eq!(plugin.skills[0].name, "plugin-skill");
        assert!(plugin.mcp_servers.contains_key("test-mcp"));
        assert_eq!(plugin.rules.len(), 1);
        assert!(plugin.is_enabled);
    }

    #[test]
    fn test_plugin_directory_discovery_with_bundled_skills_and_rules() {
        let temp = tempdir().unwrap();
        let ws_plugins = temp.path().join(".maukode").join("plugins");
        let my_plugin_dir = ws_plugins.join("bundle-plugin");
        let skills_dir = my_plugin_dir.join("skills");
        let rules_dir = my_plugin_dir.join("rules");

        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::create_dir_all(&rules_dir).unwrap();

        let manifest_json = r#"{
            "name": "bundle-plugin",
            "version": "0.1.0",
            "description": "Bundled plugin with files",
            "is_enabled": true
        }"#;
        std::fs::write(my_plugin_dir.join("maukode-plugin.json"), manifest_json).unwrap();

        let skill_md = r#"---
name: extra-skill
description: Extra bundled skill
---
Do extra work.
"#;
        std::fs::write(skills_dir.join("extra.md"), skill_md).unwrap();
        std::fs::write(rules_dir.join("security.md"), "Never leak credentials.").unwrap();

        let manager = PluginManager::new();
        let plugins = manager.list_plugins(Some(temp.path()));

        assert_eq!(plugins.len(), 1);
        let p = &plugins[0];
        assert_eq!(p.name, "bundle-plugin");
        assert_eq!(p.skills.len(), 1);
        assert_eq!(p.skills[0].name, "extra-skill");
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.rules[0], "Never leak credentials.");

        let enabled_skills = manager.enabled_skills(Some(temp.path()));
        assert_eq!(enabled_skills.len(), 1);
        assert_eq!(enabled_skills[0].name, "extra-skill");

        let enabled_rules = manager.enabled_rules(Some(temp.path()));
        assert_eq!(enabled_rules.len(), 1);
        assert_eq!(enabled_rules[0], "Never leak credentials.");
    }

    #[test]
    fn test_plugin_enable_disable_toggle() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("togglable");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let manifest_json = r#"{
            "name": "togglable",
            "version": "1.0.0",
            "description": "Togglable plugin",
            "is_enabled": true
        }"#;
        std::fs::write(plugin_dir.join("plugin.json"), manifest_json).unwrap();

        let mut manager = PluginManager::new();
        let plugin = manager.load_plugin_from_dir(&plugin_dir).unwrap();
        assert!(manager.is_plugin_enabled(&plugin));

        manager.set_plugin_enabled("togglable", false);
        assert!(!manager.is_plugin_enabled(&plugin));

        manager.set_plugin_enabled("togglable", true);
        assert!(manager.is_plugin_enabled(&plugin));
    }
}
