pub mod bash;
pub mod file;
pub mod grep;
pub mod registry;
pub mod security;
pub mod tool;

pub use bash::BashTool;
pub use file::{EditFileTool, GlobTool, ReadFileTool, WriteFileTool};
pub use grep::GrepTool;
pub use registry::ToolRegistry;
pub use security::{assess_bash_command, PathJail, RiskAssessment, RiskLevel};
pub use tool::{Tool, ToolDefinition, ToolError};

use std::path::PathBuf;
use std::sync::Arc;

pub fn create_default_registry(cwd: impl Into<PathBuf>) -> ToolRegistry {
    let cwd = cwd.into();
    let jail = PathJail::new(&cwd);
    create_default_registry_with_jail(cwd, jail)
}

pub fn create_default_registry_with_jail(cwd: impl Into<PathBuf>, jail: PathJail) -> ToolRegistry {
    let cwd = cwd.into();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BashTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(ReadFileTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(WriteFileTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(EditFileTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GlobTool::with_jail(cwd.clone(), jail.clone())));
    registry.register(Arc::new(GrepTool::with_jail(cwd, jail)));
    registry
}
