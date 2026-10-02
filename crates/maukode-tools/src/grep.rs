use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use crate::tool::{Tool, ToolError};

pub struct GrepTool {
    pub base_dir: PathBuf,
}

impl GrepTool {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
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
            Some(ref p) => {
                let path = Path::new(p);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.base_dir.join(path)
                }
            }
            None => self.base_dir.clone(),
        };

        let mut results = Vec::new();
        let max_matches = 100;

        for entry in walkdir::WalkDir::new(&search_path)
            .max_depth(10)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != "target" && name != "node_modules"
            })
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    for (line_idx, line) in content.lines().enumerate() {
                        if re.is_match(line) {
                            results.push(format!(
                                "{}:{}: {}",
                                entry.path().display(),
                                line_idx + 1,
                                line.trim_end()
                            ));
                            if results.len() >= max_matches {
                                results.push("... (results truncated to 100 matches)".to_string());
                                break;
                            }
                        }
                    }
                }
            }
            if results.len() >= max_matches {
                break;
            }
        }

        if results.is_empty() {
            Ok(format!("No matches found for '{}'", args.query))
        } else {
            Ok(results.join("\n"))
        }
    }
}
