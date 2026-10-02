pub mod formatter;
pub mod graph;
pub mod parser;
pub mod tool;

use anyhow::Result;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

pub use formatter::RepoMapFormatter;
pub use graph::{FileSymbolData, RankedFile, SymbolGraph};
pub use parser::{AstParser, ParsedFileSymbols, SymbolDef, SymbolKind};
pub use tool::CodebaseMapTool;

#[derive(Clone)]
pub struct RepoMapGenerator {
    workspace_root: PathBuf,
    cache: Arc<RwLock<HashMap<PathBuf, (SystemTime, ParsedFileSymbols)>>>,
}

impl RepoMapGenerator {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let root = workspace_root.into();
        let canonical_root = root.canonicalize().unwrap_or(root);
        Self {
            workspace_root: canonical_root,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Generates a compact codebase skeleton map fitting within `max_tokens`.
    pub async fn generate_map(
        &self,
        max_tokens: usize,
        focus_path: Option<&str>,
        query: Option<&str>,
    ) -> Result<String> {
        let root = self.workspace_root.clone();
        let cache_arc = Arc::clone(&self.cache);

        // Run file traversal & parsing in a blocking task so we don't stall Tokio executor
        let file_data = tokio::task::spawn_blocking(move || {
            let mut parser = match AstParser::new() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Failed to initialize AstParser: {e}");
                    return Vec::new();
                }
            };

            let mut files_to_process = Vec::new();
            let walker = WalkBuilder::new(&root)
                .hidden(true)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .filter_entry(|entry| {
                    let file_name = entry.file_name().to_string_lossy();
                    // Skip bulky folders
                    !matches!(
                        file_name.as_ref(),
                        "target"
                            | "node_modules"
                            | ".git"
                            | ".cargo"
                            | "dist"
                            | "build"
                            | "vendor"
                            | "__pycache__"
                    )
                })
                .build();

            for result in walker {
                let entry = match result {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go") {
                    continue;
                }

                // Check file size (< 256KB to avoid huge generated files)
                if let Ok(meta) = entry.metadata() {
                    if meta.len() > 256 * 1024 {
                        continue;
                    }
                    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    files_to_process.push((path.to_path_buf(), mtime));
                }
            }

            let mut collected = Vec::with_capacity(files_to_process.len());

            for (path, mtime) in files_to_process {
                // Check in-memory cache
                let cached_val = {
                    let guard = cache_arc.blocking_read();
                    guard.get(&path).cloned()
                };

                let symbols = if let Some((cached_mtime, cached_symbols)) = cached_val {
                    if cached_mtime == mtime {
                        Some(cached_symbols)
                    } else {
                        parse_and_cache(&mut parser, &path, mtime, &cache_arc)
                    }
                } else {
                    parse_and_cache(&mut parser, &path, mtime, &cache_arc)
                };

                if let Some(syms) = symbols {
                    if let Ok(rel) = path.strip_prefix(&root) {
                        collected.push(FileSymbolData {
                            relative_path: rel.to_path_buf(),
                            definitions: syms.definitions,
                            references: syms.references,
                        });
                    }
                }
            }

            collected
        })
        .await?;

        // Rank files based on reference graph / PageRank & query
        let ranked = SymbolGraph::rank_files(&file_data, query, focus_path);

        // Format into concise outline within token budget
        let output = RepoMapFormatter::format(&ranked, max_tokens);

        Ok(output)
    }
}

fn parse_and_cache(
    parser: &mut AstParser,
    path: &Path,
    mtime: SystemTime,
    cache_arc: &Arc<RwLock<HashMap<PathBuf, (SystemTime, ParsedFileSymbols)>>>,
) -> Option<ParsedFileSymbols> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    let parsed = parser.parse_file(path, &content)?;
    let mut guard = cache_arc.blocking_write();
    guard.insert(path.to_path_buf(), (mtime, parsed.clone()));
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_rust_parser_ast() {
        let mut parser = AstParser::new().unwrap();
        let code = r#"
            pub struct SessionRunner {
                id: String,
            }

            pub enum Status {
                Active,
                Done,
            }

            impl SessionRunner {
                pub fn new() -> Self {
                    SessionRunner { id: String::new() }
                }

                pub async fn run(&self) -> Result<()> {
                    Ok(())
                }
            }
        "#;

        let parsed = parser.parse_file(Path::new("runner.rs"), code).unwrap();
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "SessionRunner" && d.kind == SymbolKind::Struct));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "Status" && d.kind == SymbolKind::Enum));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "new" && d.kind == SymbolKind::Method));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "run" && d.kind == SymbolKind::Method));
    }

    #[test]
    fn test_typescript_parser_ast() {
        let mut parser = AstParser::new().unwrap();
        let code = r#"
            export interface UserConfig {
                apiUrl: string;
                timeout: number;
            }

            export class ApiClient {
                constructor(private config: UserConfig) {}

                async fetchData(): Promise<string> {
                    return "ok";
                }
            }
        "#;

        let parsed = parser.parse_file(Path::new("client.ts"), code).unwrap();
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "UserConfig" && d.kind == SymbolKind::Interface));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "ApiClient" && d.kind == SymbolKind::Class));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "fetchData" && d.kind == SymbolKind::Method));
    }

    #[test]
    fn test_python_parser_ast() {
        let mut parser = AstParser::new().unwrap();
        let code = r#"
class DataProcessor:
    def __init__(self, name: str):
        self.name = name

    def process(self, items):
        return [x * 2 for x in items]

def main():
    dp = DataProcessor("test")
"#;

        let parsed = parser.parse_file(Path::new("processor.py"), code).unwrap();
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "DataProcessor" && d.kind == SymbolKind::Class));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "process" && d.kind == SymbolKind::Method));
        assert!(parsed
            .definitions
            .iter()
            .any(|d| d.name == "main" && d.kind == SymbolKind::Function));
    }

    #[test]
    fn test_symbol_graph_and_formatter() {
        let defs1 = vec![SymbolDef {
            name: "CoreService".to_string(),
            kind: SymbolKind::Struct,
            signature: "pub struct CoreService".to_string(),
            line: 5,
            parent: None,
        }];

        let defs2 = vec![SymbolDef {
            name: "Consumer".to_string(),
            kind: SymbolKind::Struct,
            signature: "pub struct Consumer".to_string(),
            line: 10,
            parent: None,
        }];

        let mut refs2 = std::collections::HashSet::new();
        refs2.insert("CoreService".to_string());

        let files = vec![
            FileSymbolData {
                relative_path: PathBuf::from("src/core.rs"),
                definitions: defs1,
                references: std::collections::HashSet::new(),
            },
            FileSymbolData {
                relative_path: PathBuf::from("src/consumer.rs"),
                definitions: defs2,
                references: refs2,
            },
        ];

        let ranked = SymbolGraph::rank_files(&files, None, None);
        assert_eq!(ranked.len(), 2);
        // core.rs should have higher score because consumer.rs references CoreService
        assert_eq!(ranked[0].relative_path, PathBuf::from("src/core.rs"));

        let formatted = RepoMapFormatter::format(&ranked, 500);
        assert!(formatted.contains("src/core.rs:"));
        assert!(formatted.contains("pub struct CoreService"));
    }
}
