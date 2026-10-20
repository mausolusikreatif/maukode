use crate::security::PathJail;
use crate::tool::{Tool, ToolError};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

pub struct GrepTool {
    pub base_dir: PathBuf,
    pub jail: PathJail,
}

impl GrepTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base = base_dir.into();
        Self {
            jail: PathJail::new(&base),
            base_dir: base,
        }
    }

    pub fn with_jail(base_dir: impl Into<PathBuf>, jail: PathJail) -> Self {
        Self {
            base_dir: base_dir.into(),
            jail,
        }
    }
}

#[derive(Deserialize)]
struct GrepArgs {
    query: String,
    path: Option<String>,
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents for a regular expression pattern."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory or file to search within"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let args: GrepArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidParameters(e.to_string()))?;

        let re = Regex::new(&args.query)
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid regex: {e}")))?;

        let search_path = match args.path {
            Some(ref p) => self.jail.validate_path(p)?,
            None => self.jail.workspace_root.clone(),
        };
        let root = self.jail.workspace_root.clone();

        // Walking and reading files is blocking I/O; keep it off the async runtime.
        let results = tokio::task::spawn_blocking(move || search(&re, &search_path, &root))
            .await
            .map_err(|e| ToolError::Execution(format!("grep task failed: {e}")))?;

        if results.is_empty() {
            Ok(format!("No matches found for '{}'", args.query))
        } else {
            Ok(results.join("\n"))
        }
    }
}

const MAX_MATCHES: usize = 100;

/// Files larger than this are skipped: they are almost always generated or data files.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Bytes inspected for a NUL to decide that a file is binary.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// Search `path` for `re`, honouring `.gitignore` and skipping hidden, binary and large files.
/// Paths in the output are relative to `root` when they are inside it.
fn search(re: &Regex, path: &std::path::Path, root: &std::path::Path) -> Vec<String> {
    let mut results = Vec::new();
    let walker = ignore::WalkBuilder::new(path)
        .max_depth(Some(10))
        // Honour .gitignore even outside a git checkout.
        .require_git(false)
        .filter_entry(|e| {
            let name = e.file_name();
            name != "target" && name != "node_modules"
        })
        .build();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if !entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES) {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        let shown = entry.path().strip_prefix(root).unwrap_or(entry.path());
        for (line_idx, line) in content.lines().enumerate() {
            if re.is_match(line) {
                results.push(format!("{}:{}: {}", shown.display(), line_idx + 1, line.trim_end()));
                if results.len() >= MAX_MATCHES {
                    results.push(format!("... (results truncated to {MAX_MATCHES} matches)"));
                    return results;
                }
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn grep_honours_gitignore_skips_binaries_and_prints_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "needle\n").unwrap();
        std::fs::write(dir.path().join("binary.bin"), b"needle\0\x01").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn a() {}\n// needle here\n").unwrap();

        let tool = GrepTool::new(dir.path());
        let out = tool.execute(json!({ "query": "needle" })).await.unwrap();

        assert_eq!(out, "src/lib.rs:2: // needle here");
    }
}
