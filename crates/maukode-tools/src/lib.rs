pub mod bash;
pub mod compress;
pub mod exec;
pub mod file;
pub mod grep;
pub mod patch;
pub mod registry;
pub mod security;
pub mod tool;
pub mod web;

pub use bash::BashTool;
pub use exec::{
    cell_id_for, destroy_cell, format_output, CellIsolation, ExecBackend, ExecOutput, ExecRequest,
    HostBackend, LawangBackend, LawangSettings, SyncedTool,
};
pub use file::{EditFileTool, GlobTool, ReadFileTool, WriteFileTool};
pub use grep::{workspace_files, GrepTool};
pub use patch::ApplyPatchTool;
pub use registry::ToolRegistry;
pub use security::{
    assess_bash_command, bash_whitelist_allows, sandboxed_command, simple_command_words, PathJail,
    RiskAssessment, RiskLevel, SandboxMode,
};
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
    registry.register(Arc::new(ApplyPatchTool::with_jail(jail.clone())));
    registry.register(Arc::new(GlobTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GrepTool::with_jail(cwd, jail)));
    registry.register(Arc::new(WebSearchTool::new()));
    registry.register_alias("search_web", "web_search");
    registry.register(Arc::new(FetchWebContentTool::new()));
    registry.register_alias("fetch_url", "fetch_web_content");
    registry.register_alias("read_url", "fetch_web_content");
    registry
}

/// The default tools with `bash` running through `backend`, and the file tools wrapped so the
/// backend can keep its own copy of the project in step with the host's.
pub fn create_registry_with_backend(
    cwd: impl Into<PathBuf>,
    jail: PathJail,
    backend: Arc<dyn ExecBackend>,
) -> ToolRegistry {
    let cwd = cwd.into();
    let mut registry = create_default_registry_with_jail(cwd.clone(), jail.clone());
    registry.unregister("bash");
    registry.register(Arc::new(BashTool::with_backend(
        cwd,
        jail.clone(),
        backend.clone(),
    )));
    wrap_file_tools(
        &mut registry,
        &jail,
        &backend,
        &[
            ("write_file", true),
            ("edit_file", true),
            ("apply_patch", true),
        ],
    );
    registry
}

/// The read-only tools, wrapped for a backend that has its own copy of the project.
pub fn create_readonly_registry_with_backend(
    cwd: impl Into<PathBuf>,
    jail: PathJail,
    backend: Arc<dyn ExecBackend>,
) -> ToolRegistry {
    let mut registry = create_readonly_registry_with_jail(cwd, jail.clone());
    wrap_file_tools(&mut registry, &jail, &backend, &[]);
    registry
}

/// Put `SyncedTool` around the file tools. Names in `writes` push the file they changed.
fn wrap_file_tools(
    registry: &mut ToolRegistry,
    jail: &PathJail,
    backend: &Arc<dyn ExecBackend>,
    writes: &[(&str, bool)],
) {
    for name in [
        "read_file",
        "glob",
        "grep",
        "write_file",
        "edit_file",
        "apply_patch",
    ] {
        let Some(tool) = registry.get(name) else {
            continue;
        };
        let writes = writes.iter().any(|(n, w)| *n == name && *w);
        registry.unregister(name);
        registry.register(Arc::new(SyncedTool::new(
            tool,
            backend.clone(),
            jail.clone(),
            writes,
        )));
    }
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
