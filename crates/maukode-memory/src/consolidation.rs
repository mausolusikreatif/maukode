//! Memory consolidation: detect near-duplicate memories and propose merges.
//!
//! This module provides the data structures and store-level query support for finding
//! candidate duplicate pairs. The actual LLM-powered reconciliation decision (merge,
//! supersedes, or keep-separate) is implemented in [`maukode_core::consolidation`], which
//! receives [`DuplicatePair`]s from [`MemoryStore::find_duplicates`] and returns
//! [`ConsolidationSuggestion`]s.

use crate::error::Result;
use crate::models::Memory;
use crate::scope::Scope;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// A pair of memories that share enough textual overlap to be considered potential duplicates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicatePair {
    /// The older memory of the pair.
    pub older: Memory,
    /// The newer memory of the pair.
    pub newer: Memory,
    /// Jaccard similarity of tokenized titles (0.0–1.0).
    pub title_similarity: f64,
    /// Jaccard similarity of tokenized content (0.0–1.0).
    pub content_similarity: f64,
}

impl DuplicatePair {
    /// Combined similarity score used for ranking candidates.
    pub fn combined_score(&self) -> f64 {
        self.title_similarity * 0.6 + self.content_similarity * 0.4
    }

    /// `true` when either title or content exceeds the high-similarity threshold.
    pub fn is_high_similarity(&self) -> bool {
        self.title_similarity >= 0.7 || self.content_similarity >= 0.7
    }
}

/// What the reconciler decided to do with a [`DuplicatePair`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ConsolidationAction {
    /// Merge `newer` into `older`, then tombstone `newer`. The merged content replaces `older`.
    Merge {
        merged_title: String,
        merged_content: String,
        merged_importance: i64,
    },
    /// Mark `newer` as superseding `older`. `older` gets a `supersedes` backlink.
    Supersede { reason: String },
    /// Keep both memories as-is; they are distinct despite textual overlap.
    KeepSeparate { reason: String },
}

/// A single reconciliation decision output by the LLM reconciler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidationSuggestion {
    pub pair: DuplicatePair,
    pub action: ConsolidationAction,
}

/// Summary of a consolidation pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsolidationReport {
    pub pairs_examined: usize,
    pub merged: usize,
    pub superseded: usize,
    pub kept_separate: usize,
    pub skipped: usize,
    /// Number of higher-level abstraction memories created (Tier 4).
    pub abstractions_created: usize,
}

impl ConsolidationReport {
    pub fn total_actions(&self) -> usize {
        self.merged + self.superseded + self.kept_separate
    }
}

/// Tokenize a string into a set of lowercase alphanumeric tokens (length > 2).
fn tokenize(s: &str) -> std::collections::HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(str::to_string)
        .collect()
}

/// Jaccard similarity between two strings' token sets.
#[cfg(test)]
fn jaccard(a: &str, b: &str) -> f64 {
    jaccard_set(&tokenize(a), &tokenize(b))
}

/// Jaccard similarity between two pre-computed token sets.
fn jaccard_set(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    inter / union
}

impl crate::store::MemoryStore {
    /// Find all pairs of live memories within `scope` whose title or content Jaccard similarity
    /// exceeds `threshold` (default 0.5). Pairs are ordered by combined similarity score,
    /// highest first.
    ///
    /// This loads all live memories into memory for pairwise comparison. For the expected
    /// scale of MauKode's SQLite store (hundreds to low thousands of memories) this is
    /// fast enough; a future optimization could use FTS5 n-gram pre-filtering.
    pub async fn find_duplicates(
        &self,
        scope: &Scope,
        threshold: f64,
    ) -> Result<Vec<DuplicatePair>> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();

        let mut stmt = conn.prepare(
            "SELECT id, scope, taxonomy, title, content, version, author, source,
                    importance, created_at, updated_at, deleted_at
             FROM memories WHERE scope = ?1 AND deleted_at IS NULL
             ORDER BY created_at ASC",
        )?;

        let rows = stmt.query_map(params![scope_key], |row| {
            let created: String = row.get(9)?;
            let updated: String = row.get(10)?;
            let deleted: Option<String> = row.get(11)?;
            Ok(Memory {
                id: row.get(0)?,
                scope: row.get(1)?,
                taxonomy: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                version: row.get(5)?,
                author: row.get(6)?,
                source: row.get(7)?,
                importance: row.get(8)?,
                created_at: crate::store::parse_ts(&created),
                updated_at: crate::store::parse_ts(&updated),
                deleted_at: deleted.map(|d| crate::store::parse_ts(&d)),
            })
        })?;

        let mut memories: Vec<Memory> = Vec::new();
        for row in rows {
            memories.push(row?);
        }

        drop(stmt);
        drop(conn);

        // Pre-compute token sets for each memory to avoid redundant tokenization.
        let token_sets: Vec<(
            std::collections::HashSet<String>,
            std::collections::HashSet<String>,
        )> = memories
            .iter()
            .map(|m| (tokenize(&m.title), tokenize(&m.content)))
            .collect();

        let mut pairs = Vec::new();
        for i in 0..memories.len() {
            for j in (i + 1)..memories.len() {
                let (ta_title, ta_content) = &token_sets[i];
                let (tb_title, tb_content) = &token_sets[j];

                let title_sim = jaccard_set(ta_title, tb_title);
                let content_sim = jaccard_set(ta_content, tb_content);

                // Skip pairs where neither title nor content exceeds the threshold.
                if title_sim < threshold && content_sim < threshold {
                    continue;
                }

                let (older, newer) = if memories[i].created_at <= memories[j].created_at {
                    (&memories[i], &memories[j])
                } else {
                    (&memories[j], &memories[i])
                };

                pairs.push(DuplicatePair {
                    older: older.clone(),
                    newer: newer.clone(),
                    title_similarity: title_sim,
                    content_similarity: content_sim,
                });
            }
        }

        pairs.sort_by(|a, b| {
            b.combined_score()
                .partial_cmp(&a.combined_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(pairs)
    }

    /// Execute a consolidation suggestion: perform the merge/supersede/keep action.
    pub async fn apply_consolidation(
        &self,
        scope: &Scope,
        suggestion: &ConsolidationSuggestion,
        author: Option<&str>,
    ) -> Result<()> {
        match &suggestion.action {
            ConsolidationAction::Merge {
                merged_title,
                merged_content,
                merged_importance,
            } => {
                // First, append a supersedes link to the newer memory's content so the
                // edge graph records the relationship after tombstoning.
                let link_text =
                    format!("\n\n[consolidated] Superseded by [[{}]].", merged_title);
                self.update(
                    scope,
                    &suggestion.pair.newer.id,
                    None,
                    None,
                    Some(&format!("{}{}", suggestion.pair.newer.content, link_text)),
                    None,
                    Some(suggestion.pair.newer.version),
                    author,
                )
                .await?;

                // Tombstone the newer memory before updating the older one, so there is
                // no title-clash if merged_title happens to equal newer.title.
                self.delete(scope, &suggestion.pair.newer.id).await?;

                // Now update the older memory with merged content.
                self.update(
                    scope,
                    &suggestion.pair.older.id,
                    None,
                    Some(merged_title),
                    Some(merged_content),
                    Some(*merged_importance),
                    Some(suggestion.pair.older.version),
                    author,
                )
                .await?;
            }
            ConsolidationAction::Supersede { .. } => {
                // Add a supersedes link from newer pointing at older.
                let link_text = format!(
                    "\n\n[consolidated] Supersedes [[{}]].",
                    suggestion.pair.older.title
                );
                self.update(
                    scope,
                    &suggestion.pair.newer.id,
                    None,
                    None,
                    Some(&format!("{}{}", suggestion.pair.newer.content, link_text)),
                    None,
                    Some(suggestion.pair.newer.version),
                    author,
                )
                .await?;
            }
            ConsolidationAction::KeepSeparate { .. } => {
                // No action needed.
            }
        }
        Ok(())
    }

    /// Count all live memories in a scope. Useful for deciding whether consolidation is
    /// worthwhile before running the full pairwise scan.
    pub async fn count_live(&self, scope: &Scope) -> Result<usize> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE scope = ?1 AND deleted_at IS NULL",
            params![scope_key],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// List all live memories in a scope, ordered by taxonomy then updated_at.
    /// Used by the abstraction engine to feed related memories to the LLM.
    pub async fn list_live(&self, scope: &Scope) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();
        let mut stmt = conn.prepare(
            "SELECT id, scope, taxonomy, title, content, version, author, source,
                    importance, created_at, updated_at, deleted_at
             FROM memories
             WHERE scope = ?1 AND deleted_at IS NULL
             ORDER BY taxonomy, updated_at",
        )?;
        let rows = stmt.query_map(params![scope_key], |row| {
            let created: String = row.get(9)?;
            let updated: String = row.get(10)?;
            let deleted: Option<String> = row.get(11)?;
            Ok(Memory {
                id: row.get(0)?,
                scope: row.get(1)?,
                taxonomy: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                version: row.get(5)?,
                author: row.get(6)?,
                source: row.get(7)?,
                importance: row.get(8)?,
                created_at: crate::store::parse_ts(&created),
                updated_at: crate::store::parse_ts(&updated),
                deleted_at: deleted.map(|d| crate::store::parse_ts(&d)),
            })
        })?;
        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }
        Ok(memories)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn jaccard_basic() {
        assert!((jaccard("hello world foo", "hello world bar") - 2.0 / 4.0).abs() < 0.01);
        assert_eq!(jaccard("abc", "xyz"), 0.0);
        assert_eq!(jaccard("", ""), 0.0);
    }

    #[test]
    fn duplicate_pair_combined_score() {
        let pair = DuplicatePair {
            older: Memory {
                id: "a".into(),
                scope: "global".into(),
                taxonomy: "x".into(),
                title: "t".into(),
                content: "c".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 3,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
            },
            newer: Memory {
                id: "b".into(),
                scope: "global".into(),
                taxonomy: "x".into(),
                title: "t".into(),
                content: "c".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 3,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
            },
            title_similarity: 0.8,
            content_similarity: 0.5,
        };
        assert!((pair.combined_score() - (0.8 * 0.6 + 0.5 * 0.4)).abs() < 0.01);
        assert!(pair.is_high_similarity());
    }

    #[tokio::test]
    async fn list_live_and_count_live() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("mem.db")).unwrap();
        let scope = Scope::global();

        let res_a = store
            .create(&scope, "gotchas", "Alpha", "content a", 3, None, "tool")
            .await
            .unwrap();
        let res_b = store
            .create(&scope, "gotchas", "Beta", "content b", 3, None, "tool")
            .await
            .unwrap();

        // Soft-delete "Beta".
        store.delete(&scope, &res_b.memory.id).await.unwrap();

        let live = store.list_live(&scope).await.unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].title, "Alpha");

        let count = store.count_live(&scope).await.unwrap();
        assert_eq!(count, 1);
    }
}
