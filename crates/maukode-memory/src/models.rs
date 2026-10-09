use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single stored memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub scope: String,
    pub taxonomy: String,
    pub title: String,
    pub content: String,
    pub version: i64,
    pub author: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Memory {
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

/// A historical snapshot of a memory, appended on every create/update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRevision {
    pub memory_id: String,
    pub version: i64,
    pub title: String,
    pub content: String,
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Link relations carried by `[[rel::Title]]`. Unknown prefixes degrade to `Relates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkRelation {
    Relates,
    DependsOn,
    Supersedes,
    Contradicts,
}

impl LinkRelation {
    pub fn as_str(&self) -> &'static str {
        match self {
            LinkRelation::Relates => "relates",
            LinkRelation::DependsOn => "depends-on",
            LinkRelation::Supersedes => "supersedes",
            LinkRelation::Contradicts => "contradicts",
        }
    }

    /// Parse a relation prefix. Unknown prefixes return `None` so the caller can treat the whole
    /// bracket text as a plain `relates` target (graceful degradation).
    pub fn parse(prefix: &str) -> Option<Self> {
        match prefix {
            "relates" => Some(LinkRelation::Relates),
            "depends-on" => Some(LinkRelation::DependsOn),
            "supersedes" => Some(LinkRelation::Supersedes),
            "contradicts" => Some(LinkRelation::Contradicts),
            _ => None,
        }
    }

    /// Whether an inbound edge of this relation counts as "authority" for ranking.
    pub fn is_authority(&self) -> bool {
        matches!(self, LinkRelation::Relates | LinkRelation::DependsOn)
    }
}

/// A directed edge from a memory to a target (resolved or dangling).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub scope: String,
    pub src_id: String,
    pub dst_id: Option<String>,
    pub raw_target: String,
    pub rel: LinkRelation,
}

/// A search result row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub taxonomy: String,
    pub content: String,
    pub score: f64,
    pub superseded: bool,
}

/// Search response including the token-budget truncation accounting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchHit>,
    pub omitted_count: usize,
}

/// One taxonomy path with a count of live memories filed there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub path: String,
    pub count: usize,
    pub expandable: bool,
}

/// Title-level browse row (no content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseEntry {
    pub id: String,
    pub title: String,
    pub taxonomy: String,
}

/// A declared "what goes here" branch of the corpus taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxonomyBranch {
    pub path: String,
    pub description: String,
}

/// Store-wide link health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EdgeHealth {
    pub total: usize,
    pub resolved: usize,
    pub dangling: usize,
    pub top_dangling: Vec<DanglingTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DanglingTarget {
    pub raw_target: String,
    pub count: usize,
}

/// A pair of memories with strong textual overlap but no edge between them yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedLink {
    pub src_id: String,
    pub src_title: String,
    pub dst_id: String,
    pub dst_title: String,
    pub overlap_score: f64,
}

/// A node in the link graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub taxonomy: String,
    pub authority: usize,
    pub superseded: bool,
    pub dangling: bool,
}

/// An edge in the link graph (targets may be dangling ghost nodes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub src_id: String,
    pub dst_id: String,
    pub rel: LinkRelation,
}

/// A resolved memory plus its outgoing links, returned on write/update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteResult {
    pub memory: Memory,
    pub links: Vec<EdgeStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeStatus {
    pub raw_target: String,
    pub rel: LinkRelation,
    pub resolved: bool,
    pub dst_id: Option<String>,
}
