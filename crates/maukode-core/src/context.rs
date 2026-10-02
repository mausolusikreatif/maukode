use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::repo_map::RepoMapGenerator;

pub struct ContextManager {
    cwd: PathBuf,
    repo_map: Arc<RepoMapGenerator>,
}

impl ContextManager {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let repo_map = Arc::new(RepoMapGenerator::new(&cwd));
        Self { cwd, repo_map }
    }

    pub fn repo_map(&self) -> &Arc<RepoMapGenerator> {
        &self.repo_map
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Discover project instruction files (e.g., AGENTS.md or .maukoderc) upwards
    pub async fn discover_instructions(&self) -> Option<String> {
        let mut curr = self.cwd.clone();
        loop {
            let agents_file = curr.join("AGENTS.md");
            if agents_file.is_file() {
                if let Ok(content) = tokio::fs::read_to_string(&agents_file).await {
                    return Some(content);
                }
            }
            if !curr.pop() {
                break;
            }
        }
        None
    }

    /// Render baseline system context with project instructions and codebase repository map
    pub async fn render_system_context(&self) -> String {
        let now = Utc::now().to_rfc3339();
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let cwd_str = self.cwd.display().to_string();

        let mut prompt = format!(
            "You are MauKode, an expert agentic AI coding assistant designed to solve complex software engineering tasks.\n\
             Environment:\n\
             - Operating System: {os} ({arch})\n\
             - Working Directory: {cwd_str}\n\
             - Current Time: {now}\n\n\
             Guidelines:\n\
             - Always inspect existing files before making modifications.\n\
             - Use the provided tools (read_file, write_file, edit_file, bash, grep, glob, get_codebase_map) to explore and make changes.\n\
             - Keep responses concise and focused on the solution.\n\
             - Prefer surgical edits using `edit_file` over overwriting complete files when possible.\n"
        );

        if let Some(instructions) = self.discover_instructions().await {
            prompt.push_str("\n--- Project Instructions (from AGENTS.md) ---\n");
            prompt.push_str(&instructions);
            prompt.push_str("\n---------------------------------------------\n");
        }

        // Generate and inject concise codebase skeleton map
        if let Ok(map) = self.repo_map.generate_map(800, None, None).await {
            if !map.trim().is_empty() {
                prompt.push_str("\n--- Codebase Map (Tree-Sitter Symbol Skeleton) ---\n");
                prompt.push_str(&map);
                prompt.push_str("\n--------------------------------------------------\n");
            }
        }

        prompt
    }
}
