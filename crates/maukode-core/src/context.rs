use crate::repo_map::RepoMapGenerator;
use crate::rules::ProjectMemory;
use crate::store::sqlite::SqliteSessionStore;
use chrono::Utc;
use maukode_memory::{MemoryContext, MemoryStore, Scope};
use maukode_tools::ToolRegistry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A memory store bound to a scope, plus whether end-of-turn extraction is enabled. The runner
/// and context manager share one of these; the tools use [`MemoryContext`] directly.
#[derive(Clone)]
pub struct MemoryBinding {
    pub context: MemoryContext,
    pub auto_extract: bool,
}

impl MemoryBinding {
    pub fn new(store: Arc<MemoryStore>, scope: Scope) -> Self {
        Self {
            context: MemoryContext::new(store, scope),
            auto_extract: false,
        }
    }

    /// Open the shared `maukode.db` and bind it to `scope`. Returns an error if the database
    /// cannot be opened; callers usually degrade gracefully by skipping memory rather than failing.
    pub fn open_default(scope: Scope) -> anyhow::Result<Self> {
        let store = Arc::new(MemoryStore::open(SqliteSessionStore::default_db_path())?);
        Ok(Self::new(store, scope))
    }

    pub fn with_auto_extract(mut self, enabled: bool) -> Self {
        self.auto_extract = enabled;
        self
    }

    pub fn store(&self) -> &Arc<MemoryStore> {
        &self.context.store
    }

    pub fn scope(&self) -> &Scope {
        &self.context.scope
    }
}

/// Register the memory tools for `binding` into `registry`.
pub fn register_memory_tools(registry: &mut ToolRegistry, binding: &MemoryBinding) {
    maukode_memory::register_tools(registry, binding.context.clone());
}

/// Whether automatic end-of-turn extraction is enabled, from `MAUKODE_MEMORY_AUTO_EXTRACT`.
pub fn memory_auto_extract_enabled() -> bool {
    std::env::var("MAUKODE_MEMORY_AUTO_EXTRACT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Build a context for `dir` with the project-scoped memory store attached when it can be opened.
/// Also registers the memory tools into `registry` and returns the binding for the runner. When
/// the store cannot be opened the context works exactly as before and `None` is returned.
pub fn context_with_project_memory(
    dir: impl AsRef<Path>,
    registry: &mut ToolRegistry,
) -> (ContextManager, Option<MemoryBinding>) {
    let (context, binding) = project_context(dir);
    if let Some(binding) = &binding {
        register_memory_tools(registry, binding);
    }
    (context, binding)
}

/// Like [`context_with_project_memory`] without registering tools, for callers that keep the
/// context across turns (its repo-map cache then survives) and register the tools per turn.
pub fn project_context(dir: impl AsRef<Path>) -> (ContextManager, Option<MemoryBinding>) {
    let dir = dir.as_ref();
    let binding = match MemoryBinding::open_default(Scope::project(dir)) {
        Ok(binding) => Some(binding.with_auto_extract(memory_auto_extract_enabled())),
        Err(e) => {
            tracing::warn!("Memory store unavailable, continuing without it: {e}");
            None
        }
    };
    let context = match &binding {
        Some(binding) => ContextManager::new(dir).with_knowledge(binding.clone()),
        None => ContextManager::new(dir),
    };
    (context, binding)
}

pub struct ContextManager {
    cwd: PathBuf,
    repo_map: Arc<RepoMapGenerator>,
    memory: Arc<ProjectMemory>,
    knowledge: Option<MemoryBinding>,
}

impl ContextManager {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let repo_map = Arc::new(RepoMapGenerator::new(&cwd));
        let memory = Arc::new(ProjectMemory::new(&cwd));
        Self {
            cwd,
            repo_map,
            memory,
            knowledge: None,
        }
    }

    /// Attach a knowledge store so the system prompt can carry the taxonomy template and relevant
    /// recall hits. Without this, memory tools are still usable but nothing is auto-injected.
    pub fn with_knowledge(mut self, binding: MemoryBinding) -> Self {
        self.knowledge = Some(binding);
        self
    }

    pub fn knowledge(&self) -> Option<&MemoryBinding> {
        self.knowledge.as_ref()
    }

    pub fn repo_map(&self) -> &Arc<RepoMapGenerator> {
        &self.repo_map
    }

    pub fn memory(&self) -> &Arc<ProjectMemory> {
        &self.memory
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Discover project and global instruction files (e.g., AGENTS.md, MAUKODE.md, CLAUDE.md)
    pub async fn discover_instructions(&self) -> Option<String> {
        let merged = self.memory.render_merged_rules().await;
        if merged.is_empty() {
            None
        } else {
            Some(merged)
        }
    }

    /// Render the taxonomy template plus top-ranked recall hits for `query`, within a bounded
    /// token budget. Deterministic (FTS5 only); returns an empty string when no store is attached.
    pub async fn render_knowledge_context(&self, query: Option<&str>) -> String {
        let Some(binding) = &self.knowledge else {
            return String::new();
        };
        let scope = binding.scope();

        let mut out = String::new();
        if let Ok(branches) = binding.store().taxonomy_template(scope).await {
            if !branches.is_empty() {
                out.push_str(
                    "\n--- Memory Taxonomy (file new memories under these branches) ---\n",
                );
                for branch in branches {
                    out.push_str(&format!("- {}: {}\n", branch.path, branch.description));
                }
            }
        }

        if let Some(query) = query.filter(|q| !q.trim().is_empty()) {
            if let Ok(results) = binding.store().search(scope, query, None, Some(1500)).await {
                if !results.results.is_empty() {
                    out.push_str("\n--- Relevant Memories (recent recall) ---\n");
                    for hit in &results.results {
                        out.push_str(&format!("- [{}] {}: {}\n", hit.id, hit.title, hit.content));
                    }
                }
            }
        }

        if out.is_empty() {
            out
        } else {
            out.push_str(
                "---------------------------------------------------------\n\
                 Use `memory_search` to recall more, and `memory_write` to store durable facts.",
            );
            out
        }
    }

    /// The part of the system prompt that changes between turns: today's date and the knowledge
    /// recalled for `query`. Providers that cache prompts get it after the stable part.
    pub async fn render_turn_context(&self, query: Option<&str>) -> String {
        let mut out = format!("Current date: {}\n", Utc::now().format("%Y-%m-%d"));
        out.push_str(&self.render_knowledge_context(query).await);
        out
    }

    /// Render baseline system context with project instructions and codebase repository map.
    /// Keep it free of anything that changes from turn to turn (see [`Self::render_turn_context`]),
    /// so providers can cache it.
    pub async fn render_system_context(&self) -> String {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let cwd_str = self.cwd.display().to_string();

        let mut prompt = format!(
            "You are MauKode, an expert agentic AI coding assistant designed to solve complex software engineering tasks.\n\
             Environment:\n\
             - Operating System: {os} ({arch})\n\
             - Working Directory: {cwd_str}\n\n\
             Guidelines:\n\
             - Always inspect existing files before making modifications.\n\
             - Use the provided tools (read_file, write_file, edit_file, apply_patch, bash, grep, glob, web_search, fetch_web_content, get_codebase_map) to explore and make changes.\n\
             - When researching modern libraries, new crate versions, breaking API changes, or unfamiliar error messages, proactively use `web_search` and `fetch_web_content` to consult up-to-date documentation.\n\
             - Keep responses concise and focused on the solution.\n\
             - Prefer surgical edits using `edit_file` (one change) or `apply_patch` (changes across several files) over overwriting complete files when possible.\n\
             - A user message may end with a `<maukode-context>` note (current date, memories relevant to that request). MauKode adds it; it was not written by the user.\n\n\
             Working style:\n\
             - Finish the task end to end. Keep calling tools until the work is done and verified; do not stop to ask whether to continue or to report intermediate progress.\n\
             - Never announce a next step (\"I will now...\", \"Next, I'll...\") without performing it in the same response. If you say you will do something, make the tool call.\n\
             - Only end your turn with a plain-text reply when the task is complete, or when you are blocked and need information that only the user can provide. State clearly which of the two it is.\n\
             - If a tool call fails, diagnose and retry with a corrected approach before giving up.\n"
        );

        if let Some(instructions) = self.discover_instructions().await {
            prompt.push('\n');
            prompt.push_str(&instructions);
            prompt.push('\n');
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
