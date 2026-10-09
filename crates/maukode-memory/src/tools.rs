//! `Tool` implementations exposing the memory store to the agent through the unified registry.

use crate::models::{SearchHit, TaxonomyBranch};
use crate::scope::Scope;
use crate::store::MemoryStore;
use async_trait::async_trait;
use maukode_tools::{Tool, ToolError, ToolSource};
use serde_json::{json, Value};
use std::sync::Arc;

/// Shared context every memory tool needs: the store, the scope to operate in, and the author
/// identity recorded on writes.
#[derive(Clone)]
pub struct MemoryContext {
    pub store: Arc<MemoryStore>,
    pub scope: Scope,
    pub author: Option<String>,
}

impl MemoryContext {
    pub fn new(store: Arc<MemoryStore>, scope: Scope) -> Self {
        Self {
            store,
            scope,
            author: None,
        }
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Per-call scope override. Global is only reachable when explicitly requested; everything
    /// else stays in the context's project scope.
    fn scope_for(&self, args: &Value) -> Scope {
        match args.get("scope").and_then(|v| v.as_str()) {
            Some("global") => Scope::global(),
            _ => self.scope.clone(),
        }
    }

    fn tool_error(err: crate::error::MemoryError) -> ToolError {
        match err {
            crate::error::MemoryError::InvalidInput(m) => ToolError::InvalidParameters(m),
            crate::error::MemoryError::NotFound(m) => ToolError::NotFound(m),
            other => ToolError::Execution(other.to_string()),
        }
    }
}

/// All memory tools as boxed `Tool` trait objects for insertion into a registry.
pub fn all_tools(context: MemoryContext) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(MemorySearchTool(context.clone())),
        Arc::new(MemoryCatalogTool(context.clone())),
        Arc::new(MemoryBrowseTool(context.clone())),
        Arc::new(MemoryWriteTool(context.clone())),
        Arc::new(MemoryGetTool(context.clone())),
        Arc::new(MemoryUpdateTool(context.clone())),
        Arc::new(MemoryHistoryTool(context.clone())),
        Arc::new(MemoryRestoreTool(context.clone())),
        Arc::new(MemoryDeleteTool(context.clone())),
        Arc::new(MemoryGraphTool(context.clone())),
        Arc::new(MemoryEdgeHealthTool(context.clone())),
        Arc::new(MemorySuggestLinksTool(context.clone())),
    ]
}

/// Register all memory tools into a registry.
pub fn register_tools(registry: &mut maukode_tools::ToolRegistry, context: MemoryContext) {
    for tool in all_tools(context) {
        registry.register(tool);
    }
}

fn parse_usize(args: &Value, key: &str) -> Result<Option<usize>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_i64().ok_or_else(|| {
                ToolError::InvalidParameters(format!("'{key}' must be an integer"))
            })?;
            if n < 0 {
                return Err(ToolError::InvalidParameters(format!(
                    "'{key}' must not be negative"
                )));
            }
            Ok(Some(n as usize))
        }
    }
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidParameters(format!("'{key}' is required")))
}

fn render_hits(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "No matching memories.".to_string();
    }
    hits.iter()
        .map(|h| {
            let flag = if h.superseded { " [superseded]" } else { "" };
            format!(
                "- [{}] {} ({}, score {:.2}){}\n{}",
                h.id, h.title, h.taxonomy, h.score, flag, h.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ---- memory_search ------------------------------------------------------------------------

pub struct MemorySearchTool(pub MemoryContext);

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }
    fn description(&self) -> &str {
        "Search stored project memories by keyword. Results are ranked by full-text relevance \
         blended with link authority. Use 'taxonomy' to scope to a branch and 'token_budget' to \
         cap the response size."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Keywords to search for (required)." },
                "taxonomy": { "type": "string", "description": "Optional taxonomy branch to scope the search." },
                "token_budget": { "type": "integer", "description": "Optional approximate token budget." },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["query"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let query = required_str(&args, "query")?;
        let taxonomy = args.get("taxonomy").and_then(|v| v.as_str());
        let budget = parse_usize(&args, "token_budget")?;
        let scope = self.0.scope_for(&args);
        let result = self
            .0
            .store
            .search(&scope, query, taxonomy, budget)
            .await
            .map_err(MemoryContext::tool_error)?;
        let mut out = render_hits(&result.results);
        if result.omitted_count > 0 {
            out.push_str(&format!(
                "\n\n[{} more result(s) omitted by the token budget; narrow the query or raise token_budget.]",
                result.omitted_count
            ));
        }
        Ok(out)
    }
}

// ---- memory_catalog -----------------------------------------------------------------------

pub struct MemoryCatalogTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryCatalogTool {
    fn name(&self) -> &str {
        "memory_catalog"
    }
    fn description(&self) -> &str {
        "List taxonomy branches that actually contain memories, as a shallow drillable tree."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prefix": { "type": "string", "description": "Optional taxonomy prefix to drill into." },
                "depth": { "type": "integer", "description": "Levels to reveal (default 1)." },
                "scope": { "type": "string", "enum": ["project", "global"] }
            }
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let prefix = args.get("prefix").and_then(|v| v.as_str());
        let depth = parse_usize(&args, "depth")?.unwrap_or(1);
        let scope = self.0.scope_for(&args);
        let entries = self
            .0
            .store
            .catalog(&scope, prefix, depth)
            .await
            .map_err(MemoryContext::tool_error)?;
        if entries.is_empty() {
            return Ok("No memories filed yet.".to_string());
        }
        Ok(entries
            .iter()
            .map(|e| {
                format!(
                    "- {} ({} memor{}){}",
                    e.path,
                    e.count,
                    if e.count == 1 { "y" } else { "ies" },
                    if e.expandable { " [+]" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ---- memory_browse ------------------------------------------------------------------------

pub struct MemoryBrowseTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryBrowseTool {
    fn name(&self) -> &str {
        "memory_browse"
    }
    fn description(&self) -> &str {
        "Browse titles (no content) under a taxonomy branch, most-recently-updated first."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "taxonomy": { "type": "string", "description": "Branch to browse (required)." },
                "token_budget": { "type": "integer" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["taxonomy"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let taxonomy = required_str(&args, "taxonomy")?;
        let budget = parse_usize(&args, "token_budget")?;
        let scope = self.0.scope_for(&args);
        let entries = self
            .0
            .store
            .browse(&scope, taxonomy, budget)
            .await
            .map_err(MemoryContext::tool_error)?;
        if entries.is_empty() {
            return Ok(format!("No memories under '{taxonomy}'."));
        }
        Ok(entries
            .iter()
            .map(|e| format!("- [{}] {} ({})", e.id, e.title, e.taxonomy))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ---- memory_write -------------------------------------------------------------------------

pub struct MemoryWriteTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }
    fn description(&self) -> &str {
        "Store a new durable memory (a fact, convention, decision, or gotcha worth remembering \
         across sessions). Content may link to other memories with [[Title]] or [[rel::Title]], \
         where rel is one of relates, depends-on, supersedes, contradicts."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "taxonomy": { "type": "string", "description": "Dot-notation path, e.g. architecture.parsing." },
                "content": { "type": "string" },
                "scope": { "type": "string", "enum": ["project", "global"], "description": "Defaults to project." }
            },
            "required": ["title", "taxonomy", "content"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let title = required_str(&args, "title")?;
        let taxonomy = required_str(&args, "taxonomy")?;
        let content = required_str(&args, "content")?;
        let scope = self.0.scope_for(&args);
        let result = self
            .0
            .store
            .create(
                &scope,
                taxonomy,
                title,
                content,
                self.0.author.as_deref(),
                "tool",
            )
            .await
            .map_err(MemoryContext::tool_error)?;
        Ok(format!(
            "Stored memory '{}' (id {}, version {}, {} link(s)).",
            result.memory.title,
            result.memory.id,
            result.memory.version,
            result.links.len()
        ))
    }
}

// ---- memory_get ---------------------------------------------------------------------------

pub struct MemoryGetTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }
    fn description(&self) -> &str {
        "Fetch a memory by id, including its full content."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["id"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let id = required_str(&args, "id")?;
        let scope = self.0.scope_for(&args);
        let memory = self
            .0
            .store
            .get(&scope, id)
            .await
            .map_err(MemoryContext::tool_error)?
            .ok_or_else(|| ToolError::NotFound(id.to_string()))?;
        Ok(serde_json::to_string_pretty(&memory).unwrap_or_default())
    }
}

// ---- memory_update ------------------------------------------------------------------------

pub struct MemoryUpdateTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryUpdateTool {
    fn name(&self) -> &str {
        "memory_update"
    }
    fn description(&self) -> &str {
        "Update an existing memory. Provide base_version for optimistic concurrency; a mismatch is \
         reported as a conflict rather than overwriting a concurrent change."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "title": { "type": "string" },
                "taxonomy": { "type": "string" },
                "content": { "type": "string" },
                "base_version": { "type": "integer" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["id"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let id = required_str(&args, "id")?;
        let title = args.get("title").and_then(|v| v.as_str());
        let taxonomy = args.get("taxonomy").and_then(|v| v.as_str());
        let content = args.get("content").and_then(|v| v.as_str());
        let base_version = args.get("base_version").and_then(|v| v.as_i64());
        let scope = self.0.scope_for(&args);
        let result = self
            .0
            .store
            .update(
                &scope,
                id,
                taxonomy,
                title,
                content,
                base_version,
                self.0.author.as_deref(),
            )
            .await
            .map_err(MemoryContext::tool_error)?;
        Ok(format!(
            "Updated memory '{}' to version {}.",
            result.memory.title, result.memory.version
        ))
    }
}

// ---- memory_history -----------------------------------------------------------------------

pub struct MemoryHistoryTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryHistoryTool {
    fn name(&self) -> &str {
        "memory_history"
    }
    fn description(&self) -> &str {
        "List the revision history of a memory, newest first."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["id"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let id = required_str(&args, "id")?;
        let scope = self.0.scope_for(&args);
        let revisions = self
            .0
            .store
            .history(&scope, id)
            .await
            .map_err(MemoryContext::tool_error)?;
        Ok(revisions
            .iter()
            .map(|r| {
                format!(
                    "v{} by {} at {}: {}",
                    r.version,
                    r.author.as_deref().unwrap_or("unknown"),
                    r.created_at.to_rfc3339(),
                    r.title
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

// ---- memory_restore -----------------------------------------------------------------------

pub struct MemoryRestoreTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryRestoreTool {
    fn name(&self) -> &str {
        "memory_restore"
    }
    fn description(&self) -> &str {
        "Restore an earlier revision by copying it forward as a new version (history is preserved)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "version": { "type": "integer" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["id", "version"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let id = required_str(&args, "id")?;
        let version = args
            .get("version")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| ToolError::InvalidParameters("'version' is required".into()))?;
        let scope = self.0.scope_for(&args);
        let result = self
            .0
            .store
            .restore(&scope, id, version, self.0.author.as_deref())
            .await
            .map_err(MemoryContext::tool_error)?;
        Ok(format!(
            "Restored memory '{}' as version {}.",
            result.memory.title, result.memory.version
        ))
    }
}

// ---- memory_delete ------------------------------------------------------------------------

pub struct MemoryDeleteTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryDeleteTool {
    fn name(&self) -> &str {
        "memory_delete"
    }
    fn description(&self) -> &str {
        "Tombstone a memory. History is retained and inbound links keep pointing at the burned id."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            },
            "required": ["id"]
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let id = required_str(&args, "id")?;
        let scope = self.0.scope_for(&args);
        let deleted = self
            .0
            .store
            .delete(&scope, id)
            .await
            .map_err(MemoryContext::tool_error)?;
        Ok(if deleted {
            format!("Tombstoned memory {id}.")
        } else {
            format!("No live memory with id {id}.")
        })
    }
}

// ---- memory_graph -------------------------------------------------------------------------

pub struct MemoryGraphTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryGraphTool {
    fn name(&self) -> &str {
        "memory_graph"
    }
    fn description(&self) -> &str {
        "Return the link graph, either corpus-wide (optionally taxonomy-filtered) or an ego graph \
         around a center memory."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "center": { "type": "string", "description": "Optional memory id to center on." },
                "taxonomy": { "type": "string" },
                "depth": { "type": "integer" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            }
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let center = args.get("center").and_then(|v| v.as_str());
        let taxonomy = args.get("taxonomy").and_then(|v| v.as_str());
        let depth = parse_usize(&args, "depth")?.unwrap_or(1);
        let scope = self.0.scope_for(&args);
        let (nodes, edges) = self
            .0
            .store
            .graph(&scope, center, taxonomy, depth)
            .await
            .map_err(MemoryContext::tool_error)?;
        let rendered = json!({ "nodes": nodes, "edges": edges });
        Ok(serde_json::to_string_pretty(&rendered).unwrap_or_default())
    }
}

// ---- memory_edge_health -------------------------------------------------------------------

pub struct MemoryEdgeHealthTool(pub MemoryContext);

#[async_trait]
impl Tool for MemoryEdgeHealthTool {
    fn name(&self) -> &str {
        "memory_edge_health"
    }
    fn description(&self) -> &str {
        "Report link health: resolved vs dangling edges and the most common unresolved targets."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {
            "scope": { "type": "string", "enum": ["project", "global"] }
        } })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let scope = self.0.scope_for(&args);
        let health = self
            .0
            .store
            .edge_health(&scope)
            .await
            .map_err(MemoryContext::tool_error)?;
        Ok(serde_json::to_string_pretty(&health).unwrap_or_default())
    }
}

// ---- memory_suggest_links -----------------------------------------------------------------

pub struct MemorySuggestLinksTool(pub MemoryContext);

#[async_trait]
impl Tool for MemorySuggestLinksTool {
    fn name(&self) -> &str {
        "memory_suggest_links"
    }
    fn description(&self) -> &str {
        "Propose pairs of memories with strong textual overlap but no link between them yet."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer" },
                "scope": { "type": "string", "enum": ["project", "global"] }
            }
        })
    }
    fn source(&self) -> ToolSource {
        ToolSource::Memory
    }
    async fn execute(&self, args: Value) -> Result<String, ToolError> {
        let limit = parse_usize(&args, "limit")?.unwrap_or(10);
        let scope = self.0.scope_for(&args);
        let suggestions = self
            .0
            .store
            .suggest_links(&scope, limit)
            .await
            .map_err(MemoryContext::tool_error)?;
        if suggestions.is_empty() {
            return Ok("No unlinked pairs with strong overlap.".to_string());
        }
        Ok(suggestions
            .iter()
            .map(|s| {
                format!(
                    "- '{}' <-> '{}' (overlap {:.2})",
                    s.src_title, s.dst_title, s.overlap_score
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Convenience: the declared taxonomy branches for a scope, for prompt injection.
pub async fn taxonomy_branches(store: &MemoryStore, scope: &Scope) -> Vec<TaxonomyBranch> {
    store.taxonomy_template(scope).await.unwrap_or_default()
}
