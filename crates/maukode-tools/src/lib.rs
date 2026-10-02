pub mod bash;
pub mod file;
pub mod grep;
pub mod registry;
pub mod tool;

pub use bash::BashTool;
pub use file::{EditFileTool, GlobTool, ReadFileTool, WriteFileTool};
pub use grep::GrepTool;
pub use registry::ToolRegistry;
pub use tool::{Tool, ToolDefinition, ToolError};

use std::path::PathBuf;
use std::sync::Arc;

pub fn create_default_registry(cwd: impl Into<PathBuf>) -> ToolRegistry {
    let cwd = cwd.into();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(BashTool::new(cwd.clone())));
    registry.register(Arc::new(ReadFileTool::new(cwd.clone())));
    registry.register(Arc::new(WriteFileTool::new(cwd.clone())));
    registry.register(Arc::new(EditFileTool::new(cwd.clone())));
    registry.register(Arc::new(GlobTool::new(cwd.clone())));
    registry.register(Arc::new(GrepTool::new(cwd)));
    registry
}
