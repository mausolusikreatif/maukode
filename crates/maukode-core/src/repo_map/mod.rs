pub mod formatter;
pub mod graph;
pub mod parser;
pub mod tool;

use anyhow::Result;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;

pub use formatter::RepoMapFormatter;
pub use graph::{FileSymbolData, RankedFile, SymbolGraph};
pub use parser::{AstParser, ParsedFileSymbols, SymbolDef, SymbolKind};
pub use tool::CodebaseMapTool;

/// A source file to parse and its modification time.
type Candidate = (PathBuf, SystemTime);

/// How much one map is allowed to cost. A huge tree is mapped from the files found first, so a
/// workspace that is not a project (a home directory) cannot hold the CPU for minutes.
#[derive(Clone, Copy)]
struct Limits {
    /// Most source files looked at.
    max_files: usize,
    /// Longest the walk and the parsing may take together.
    budget: Duration,
}

const DEFAULT_LIMITS: Limits = Limits {
    max_files: 5_000,
    budget: Duration::from_secs(5),
};

/// Whether `dir` is too broad to map for the system prompt: the filesystem root or the user's home
/// directory. A map of either walks every project the user has, and tells the model little.
pub fn is_too_broad(dir: &Path) -> bool {
    let home = dirs::home_dir();
    is_broad(dir, home.as_deref())
}

fn is_broad(dir: &Path, home: Option<&Path>) -> bool {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if dir.parent().is_none() {
        return true;
    }
    match home {
        Some(h) => h.canonicalize().unwrap_or_else(|_| h.to_path_buf()) == dir,
        None => false,
    }
}

#[derive(Clone)]
pub struct RepoMapGenerator {
    workspace_root: PathBuf,
    cache: Arc<RwLock<HashMap<PathBuf, (SystemTime, ParsedFileSymbols)>>>,
    limits: Limits,
}

impl RepoMapGenerator {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let root = workspace_root.into();
        let canonical_root = root.canonicalize().unwrap_or(root);
        Self {
            workspace_root: canonical_root,
            cache: Arc::new(RwLock::new(HashMap::new())),
            limits: DEFAULT_LIMITS,
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
        let limits = self.limits;

        // Run file traversal & parsing in a blocking task so we don't stall Tokio executor
        let file_data = tokio::task::spawn_blocking(move || {
            let mut parser = match AstParser::new() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Failed to initialize AstParser: {e}");
                    return Vec::new();
                }
            };

            let started = Instant::now();
            let files_to_process = collect_candidates(&root, &limits, started);

            let mut collected = Vec::with_capacity(files_to_process.len());

            for (path, mtime) in files_to_process {
                if started.elapsed() >= limits.budget {
                    tracing::debug!("Codebase map stopped parsing at its time budget");
                    break;
                }
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

/// The source files under `root` worth parsing, at most `limits.max_files` of them, and only for
/// as long as the time budget lasts from `started`.
fn collect_candidates(root: &Path, limits: &Limits, started: Instant) -> Vec<Candidate> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
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
        if files.len() >= limits.max_files || started.elapsed() >= limits.budget {
            tracing::debug!("Codebase map walk stopped at its limits");
            break;
        }
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
            files.push((path.to_path_buf(), mtime));
        }
    }
    files
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

    fn tree_of_rust_files(count: usize) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..count {
            let file = dir.path().join(format!("f{i}.rs"));
            std::fs::write(file, "pub struct Item {}").unwrap();
        }
        dir
    }

    #[test]
    fn the_walk_stops_at_the_file_limit() {
        let dir = tree_of_rust_files(10);
        let limits = Limits {
            max_files: 3,
            budget: Duration::from_secs(60),
        };
        let found = collect_candidates(dir.path(), &limits, Instant::now());
        assert_eq!(found.len(), 3);
        let limits = Limits {
            max_files: 100,
            budget: Duration::from_secs(60),
        };
        let found = collect_candidates(dir.path(), &limits, Instant::now());
        assert_eq!(found.len(), 10);
    }

    #[test]
    fn the_walk_stops_when_the_time_is_up() {
        let dir = tree_of_rust_files(10);
        let limits = Limits {
            max_files: 100,
            budget: Duration::ZERO,
        };
        assert!(collect_candidates(dir.path(), &limits, Instant::now()).is_empty());
    }

    #[tokio::test]
    async fn a_map_is_still_made_within_the_limits() {
        let dir = tree_of_rust_files(10);
        let mut generator = RepoMapGenerator::new(dir.path());
        generator.limits.max_files = 2;
        let map = generator.generate_map(800, None, None).await.unwrap();
        assert!(map.contains(".rs"));
        assert!(map.contains("Item"));
    }

    #[test]
    fn the_root_and_the_home_directory_are_too_broad_to_map() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        std::fs::create_dir(&project).unwrap();
        assert!(is_broad(Path::new("/"), None));
        assert!(is_broad(home.path(), Some(home.path())));
        assert!(!is_broad(&project, Some(home.path())));
        assert!(!is_broad(&project, None));
    }

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
