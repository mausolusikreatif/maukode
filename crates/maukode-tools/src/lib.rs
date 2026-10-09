pub mod bash;
pub mod file;
pub mod grep;
pub mod registry;
pub mod security;
pub mod tool;
pub mod web;

pub use bash::BashTool;
pub use file::{EditFileTool, GlobTool, ReadFileTool, WriteFileTool};
pub use grep::GrepTool;
pub use registry::ToolRegistry;
pub use security::{assess_bash_command, PathJail, RiskAssessment, RiskLevel, SandboxMode};
pub use tool::{Tool, ToolDefinition, ToolError, ToolSource};
pub use web::{html_to_clean_markdown, FetchWebContentTool, SearchResultItem, WebSearchTool};

use std::path::PathBuf;
use std::sync::Arc;

pub fn create_default_registry(cwd: impl Into<PathBuf>) -> ToolRegistry {
    let cwd = cwd.into();
    let jail = PathJail::new(&cwd);
    create_default_registry_with_jail(cwd, jail)
}

pub fn create_default_registry_with_sandbox(
    cwd: impl Into<PathBuf>,
    jail: PathJail,
    sandbox: SandboxMode,
) -> ToolRegistry {
    let cwd = cwd.into();
    let mut registry = create_default_registry_with_jail(cwd.clone(), jail.clone());
    registry.unregister("bash");
    registry.register(Arc::new(BashTool::with_sandbox(cwd, jail, sandbox)));
    registry
}

pub fn create_default_registry_with_jail(cwd: impl Into<PathBuf>, jail: PathJail) -> ToolRegistry {
    let cwd = cwd.into();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BashTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(ReadFileTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(WriteFileTool::with_jail(
        cwd.clone(),
        jail.clone(),
    )));
    registry.register(Arc::new(EditFileTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GlobTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GrepTool::with_jail(cwd, jail)));
    registry.register(Arc::new(WebSearchTool::new()));
    registry.register_alias("search_web", "web_search");
    registry.register(Arc::new(FetchWebContentTool::new()));
    registry.register_alias("fetch_url", "fetch_web_content");
    registry.register_alias("read_url", "fetch_web_content");
    registry
}

pub fn create_readonly_registry(cwd: impl Into<PathBuf>) -> ToolRegistry {
    let cwd = cwd.into();
    let jail = PathJail::new(&cwd);
    create_readonly_registry_with_jail(cwd, jail)
}

pub fn create_readonly_registry_with_jail(cwd: impl Into<PathBuf>, jail: PathJail) -> ToolRegistry {
    let cwd = cwd.into();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadFileTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GlobTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GrepTool::with_jail(cwd, jail)));
    registry.register(Arc::new(WebSearchTool::new()));
    registry.register_alias("search_web", "web_search");
    registry.register(Arc::new(FetchWebContentTool::new()));
    registry.register_alias("fetch_url", "fetch_web_content");
    registry.register_alias("read_url", "fetch_web_content");
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readonly_registry_excludes_mutating_tools() {
        let temp_dir = std::env::temp_dir();
        let registry = create_readonly_registry(temp_dir);

        assert!(registry.get("read_file").is_some());
        assert!(registry.get("grep").is_some());
        assert!(registry.get("glob").is_some());
        assert!(registry.get("web_search").is_some());
        assert!(registry.get("fetch_web_content").is_some());
        assert!(registry.get("search_web").is_some());
        assert!(registry.get("read_url").is_some());

        assert!(registry.get("write_file").is_none());
        assert!(registry.get("edit_file").is_none());
        assert!(registry.get("bash").is_none());
    }
}
