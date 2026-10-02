use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use crate::tool::{Tool, ToolError};

// Helper to resolve paths safely
fn resolve_path(base: &Path, file_path: &str) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

// -------------------------------------------------------------
// ReadFileTool
// -------------------------------------------------------------
pub struct ReadFileTool {
    pub base_dir: PathBuf,
}

impl ReadFileTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }
}

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file content with line numbers. Optionally specify start_line and end_line (1-indexed)."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional 1-indexed starting line number"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional 1-indexed ending line number"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let args: ReadArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

        let target = resolve_path(&self.base_dir, &args.path);
        let content = tokio::fs::read_to_string(&target)
            .await
            .map_err(|e| ToolError::Execution(format!("Cannot read {}: {e}", target.display())))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = args.start_line.unwrap_or(1).saturating_sub(1);
        let end = args.end_line.unwrap_or(total_lines).min(total_lines);

        if start >= total_lines {
            return Ok(format!("File has {} lines. start_line is beyond EOF.", total_lines));
        }

        let mut output = String::new();
        for (idx, line) in lines[start..end].iter().enumerate() {
            let line_no = start + idx + 1;
            output.push_str(&format!("{:4}: {}\n", line_no, line));
        }

        Ok(output)
    }
}

// -------------------------------------------------------------
// WriteFileTool
// -------------------------------------------------------------
pub struct WriteFileTool {
    pub base_dir: PathBuf,
}

impl WriteFileTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a new file or completely overwrite an existing file with the provided content."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Complete content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let args: WriteArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

        let target = resolve_path(&self.base_dir, &args.path);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Execution(format!("Failed to create directories: {e}")))?;
        }

        tokio::fs::write(&target, args.content.as_bytes())
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to write {}: {e}", target.display())))?;

        Ok(format!("Successfully wrote {} bytes to {}", args.content.len(), target.display()))
    }
}

// -------------------------------------------------------------
// EditFileTool
// -------------------------------------------------------------
pub struct EditFileTool {
    pub base_dir: PathBuf,
}

impl EditFileTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }
}

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_content: String,
    new_content: String,
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact substring of text in a file with new text."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_content": {
                    "type": "string",
                    "description": "Exact text to be replaced (must match uniquely)"
                },
                "new_content": {
                    "type": "string",
                    "description": "Replacement text"
                }
            },
            "required": ["path", "old_content", "new_content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let args: EditArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

        let target = resolve_path(&self.base_dir, &args.path);
        let content = tokio::fs::read_to_string(&target)
            .await
            .map_err(|e| ToolError::Execution(format!("Cannot read {}: {e}", target.display())))?;

        let occurrences = content.matches(&args.old_content).count();
        if occurrences == 0 {
            return Err(ToolError::Execution(
                "old_content not found in the file. Ensure exact matching including whitespace.".into(),
            ));
        }
        if occurrences > 1 {
            return Err(ToolError::Execution(format!(
                "old_content matched {occurrences} times. Must match exactly once to edit safely."
            )));
        }

        let new_content = content.replacen(&args.old_content, &args.new_content, 1);
        tokio::fs::write(&target, new_content.as_bytes())
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to write {}: {e}", target.display())))?;

        Ok(format!("Successfully edited {}", target.display()))
    }
}

// -------------------------------------------------------------
// GlobTool
// -------------------------------------------------------------
pub struct GlobTool {
    pub base_dir: PathBuf,
}

impl GlobTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }
}

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
    path: Option<String>,
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "List files in a directory matching a pattern or search recursively."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Substring or extension to match in filename, e.g. '.rs' or 'Cargo'"
                },
                "path": {
                    "type": "string",
                    "description": "Optional directory to search within (default current dir)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let args: GlobArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

        let search_dir = match args.path {
            Some(ref p) => resolve_path(&self.base_dir, p),
            None => self.base_dir.clone(),
        };

        let mut matches = Vec::new();
        for entry in walkdir::WalkDir::new(&search_dir)
            .max_depth(10)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != "target" && name != "node_modules"
            })
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path_str = entry.path().to_string_lossy();
                if path_str.contains(&args.pattern) {
                    matches.push(path_str.to_string());
                }
            }
        }

        if matches.is_empty() {
            Ok(format!("No files found matching pattern '{}'", args.pattern))
        } else {
            Ok(matches.join("\n"))
        }
    }
}
