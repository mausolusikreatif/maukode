use crate::error::{MemoryError, Result};
use crate::links::{normalize_target, parse_links};
use crate::models::*;
use crate::scope::Scope;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Default weighting applied to a memory's inbound authority when ranking search hits.
const AUTHORITY_WEIGHT: f64 = 0.5;
/// Default penalty applied to a memory that another memory supersedes.
const SUPERSEDE_PENALTY: f64 = 1.0;
/// Maximum bonus applied for the highest importance (5) in search ranking.
const IMPORTANCE_WEIGHT: f64 = 0.5;
/// Maximum bonus applied for very recent memories (<7 days old).
const RECENCY_MAX_BONUS: f64 = 0.3;
/// Recency boost decays to zero at this many days.
const RECENCY_WINDOW_DAYS: i64 = 30;
/// Fresh memories (within this many days) receive the full recency bonus.
const RECENCY_FRESH_DAYS: i64 = 7;
/// Tokens are approximated at four characters each, matching the rest of the agent.
const CHARS_PER_TOKEN: usize = 4;

/// Native, embedding-free memory store backed by the shared `maukode.db` SQLite file.
pub struct MemoryStore {
    pub(crate) conn: tokio::sync::Mutex<Connection>,
    #[allow(dead_code)]
    path: PathBuf,
}

impl MemoryStore {
    /// Open (creating if needed) the memory tables in the database at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    MemoryError::InvalidInput(format!("failed to create database directory: {e}"))
                })?;
            }
        }

        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        init_schema(&conn)?;

        Ok(Self {
            conn: tokio::sync::Mutex::new(conn),
            path,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.path
    }

    // ---- writes ---------------------------------------------------------------------------

    /// Create a new memory. Fails if the title already exists in this scope.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        scope: &Scope,
        taxonomy: &str,
        title: &str,
        content: &str,
        importance: i64,
        author: Option<&str>,
        source: &str,
    ) -> Result<WriteResult> {
        validate_title(title)?;
        validate_taxonomy(taxonomy)?;
        let importance = importance.clamp(1, 5);

        let scope_key = scope.as_key();
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let conn = self.conn.lock().await;

        let exists: Option<String> = conn
            .query_row(
                "SELECT id FROM memories WHERE scope = ?1 AND title = ?2 COLLATE NOCASE AND deleted_at IS NULL",
                params![scope_key, title],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_some() {
            return Err(MemoryError::DuplicateTitle(title.to_string()));
        }

        conn.execute(
            "INSERT INTO memories (id, scope, taxonomy, title, content, version, author, source, importance, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, ?9, ?10, NULL)",
            params![
                &id,
                scope_key,
                taxonomy,
                title,
                content,
                author,
                source,
                importance,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        insert_revision(&conn, &id, 1, title, content, author, now)?;
        let links = sync_edges(&conn, &scope_key, &id, content, now)?;
        resolve_dangling(&conn, &scope_key, &normalize_target(title), &id)?;

        let memory = Memory {
            id: id.clone(),
            scope: scope_key,
            taxonomy: taxonomy.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            version: 1,
            author: author.map(str::to_string),
            source: source.to_string(),
            importance,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        };

        Ok(WriteResult { memory, links })
    }

    /// Update an existing memory. When `base_version` is set, a mismatch is a conflict.
    /// `importance` is optional; when `Some`, it updates the 1-5 importance score.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        scope: &Scope,
        id: &str,
        taxonomy: Option<&str>,
        title: Option<&str>,
        content: Option<&str>,
        importance: Option<i64>,
        base_version: Option<i64>,
        author: Option<&str>,
    ) -> Result<WriteResult> {
        let scope_key = scope.as_key();
        let now = Utc::now();
        let conn = self.conn.lock().await;

        let current = load_memory(&conn, &scope_key, id)?
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))?;

        if let Some(expected) = base_version {
            if expected != current.version {
                return Err(MemoryError::VersionConflict {
                    expected,
                    actual: current.version,
                });
            }
        }

        let new_title = title.unwrap_or(&current.title);
        let new_taxonomy = taxonomy.unwrap_or(&current.taxonomy);
        let new_content = content.unwrap_or(&current.content);
        let new_importance = importance.unwrap_or(current.importance).clamp(1, 5);
        validate_title(new_title)?;
        validate_taxonomy(new_taxonomy)?;

        if new_title != current.title {
            let clash: Option<String> = conn
                .query_row(
                    "SELECT id FROM memories WHERE scope = ?1 AND title = ?2 COLLATE NOCASE AND deleted_at IS NULL AND id != ?3",
                    params![scope_key, new_title, id],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(MemoryError::DuplicateTitle(new_title.to_string()));
            }
        }

        let next_version = current.version + 1;
        conn.execute(
            "UPDATE memories SET taxonomy = ?1, title = ?2, content = ?3, version = ?4, author = ?5, importance = ?6, updated_at = ?7
             WHERE scope = ?8 AND id = ?9",
            params![
                new_taxonomy,
                new_title,
                new_content,
                next_version,
                author,
                new_importance,
                now.to_rfc3339(),
                scope_key,
                id,
            ],
        )?;

        insert_revision(&conn, id, next_version, new_title, new_content, author, now)?;
        let links = sync_edges(&conn, &scope_key, id, new_content, now)?;
        resolve_dangling(&conn, &scope_key, &normalize_target(new_title), id)?;

        let memory = load_memory(&conn, &scope_key, id)?
            .ok_or_else(|| MemoryError::NotFound(id.to_string()))?;
        Ok(WriteResult { memory, links })
    }

    /// Tombstone a memory. History and inbound tombstones are preserved; the title is burned.
    pub async fn delete(&self, scope: &Scope, id: &str) -> Result<bool> {
        let conn = self.conn.lock().await;
        let now = Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE memories SET deleted_at = ?1 WHERE scope = ?2 AND id = ?3 AND deleted_at IS NULL",
            params![now, scope.as_key(), id],
        )?;
        Ok(updated > 0)
    }

    // ---- reads ----------------------------------------------------------------------------

    pub async fn get(&self, scope: &Scope, id: &str) -> Result<Option<Memory>> {
        let conn = self.conn.lock().await;
        load_memory(&conn, &scope.as_key(), id)
    }

    pub async fn by_title(&self, scope: &Scope, title: &str) -> Result<Option<Memory>> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();
        conn.query_row(
            "SELECT id, scope, taxonomy, title, content, version, author, source, importance, created_at, updated_at, deleted_at
             FROM memories WHERE scope = ?1 AND title = ?2 COLLATE NOCASE AND deleted_at IS NULL",
            params![scope_key, title],
            row_to_memory,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Keyword search blending FTS5 relevance with inbound authority, importance weight, a
    /// recency boost, and a supersede penalty, truncated to `token_budget` approximate tokens.
    pub async fn search(
        &self,
        scope: &Scope,
        query: &str,
        taxonomy: Option<&str>,
        token_budget: Option<usize>,
    ) -> Result<SearchResults> {
        let query = query.trim();
        if query.is_empty() {
            return Err(MemoryError::InvalidInput("query must not be empty".into()));
        }
        let match_expr = build_match_expr(query);
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();

        let mut sql = String::from(
            "SELECT m.id, m.title, m.taxonomy, m.content, bm25(memories_fts) AS bm25_score,
                    (SELECT COUNT(*) FROM memory_edges e
                       WHERE e.scope = m.scope AND e.dst_id = m.id
                         AND e.rel IN ('relates', 'depends-on')) AS authority,
                    EXISTS(SELECT 1 FROM memory_edges e
                       WHERE e.scope = m.scope AND e.dst_id = m.id AND e.rel = 'supersedes') AS superseded,
                    m.importance, m.created_at
             FROM memories_fts
             JOIN memories m ON m.rowid = memories_fts.rowid
             WHERE memories_fts MATCH ?1 AND m.scope = ?2 AND m.deleted_at IS NULL",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(match_expr), Box::new(scope_key.clone())];
        if let Some(branch) = taxonomy.filter(|b| !b.trim().is_empty()) {
            sql.push_str(" AND (m.taxonomy = ?3 OR m.taxonomy LIKE ?4)");
            args.push(Box::new(branch.to_string()));
            args.push(Box::new(format!("{branch}.%")));
        }
        sql.push_str(" ORDER BY bm25_score LIMIT 200");

        let mut stmt = conn.prepare(&sql)?;
        let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(arg_refs.as_slice(), |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let taxonomy: String = row.get(2)?;
            let content: String = row.get(3)?;
            let rank: f64 = row.get(4)?;
            let authority: i64 = row.get(5)?;
            let superseded: i64 = row.get(6)?;
            let importance: i64 = row.get(7)?;
            let created_at: String = row.get(8)?;
            Ok((
                id, title, taxonomy, content, rank, authority, superseded, importance, created_at,
            ))
        })?;

        let now = Utc::now();
        let mut scored: Vec<SearchHit> = Vec::new();
        for row in rows {
            let (id, title, taxonomy, content, rank, authority, superseded, importance, created_at) =
                row?;
            let superseded = superseded != 0;
            // Importance weight: 1-5 maps to 0.0..1.0 bonus.
            let imp_boost = (importance - 1) as f64 / 4.0 * IMPORTANCE_WEIGHT;
            // Recency boost: memories newer than 7 days get up to 0.3; decays linearly to 0 at 30d.
            let recency = recency_boost(&created_at, now);
            let mut score = -rank + AUTHORITY_WEIGHT * authority as f64 + imp_boost + recency;
            if superseded {
                score -= SUPERSEDE_PENALTY;
            }
            scored.push(SearchHit {
                id,
                title,
                taxonomy,
                content,
                score,
                superseded,
                importance,
            });
        }
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let (results, omitted_count) = apply_token_budget(scored, token_budget);
        Ok(SearchResults {
            results,
            omitted_count,
        })
    }

    /// Aggregate live memories into a shallow, drillable taxonomy tree.
    pub async fn catalog(
        &self,
        scope: &Scope,
        prefix: Option<&str>,
        depth: usize,
    ) -> Result<Vec<CatalogEntry>> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();
        let mut stmt = conn.prepare(
            "SELECT taxonomy FROM memories WHERE scope = ?1 AND deleted_at IS NULL ORDER BY taxonomy",
        )?;
        let rows = stmt.query_map(params![scope_key], |row| row.get::<_, String>(0))?;

        let prefix = prefix.map(|p| p.trim_matches('.').to_string());
        let prefix_segments = prefix.as_deref().map(split_taxonomy).unwrap_or_default();
        let depth = depth.max(1);

        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut expandable: HashMap<String, bool> = HashMap::new();
        for row in rows {
            let path = row?;
            let segments = split_taxonomy(&path);
            if !starts_with(&segments, &prefix_segments) {
                continue;
            }
            let take = (prefix_segments.len() + depth).min(segments.len());
            if take == 0 {
                continue;
            }
            let key = segments[..take].join(".");
            *counts.entry(key.clone()).or_insert(0) += 1;
            let can_expand = segments.len() > take;
            let entry = expandable.entry(key).or_insert(false);
            *entry = *entry || can_expand;
        }

        let mut entries: Vec<CatalogEntry> = counts
            .into_iter()
            .map(|(path, count)| CatalogEntry {
                expandable: *expandable.get(&path).unwrap_or(&false),
                path,
                count,
            })
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// Title-level browse of a taxonomy branch (and sub-branches); requires a branch.
    pub async fn browse(
        &self,
        scope: &Scope,
        taxonomy: &str,
        token_budget: Option<usize>,
    ) -> Result<Vec<BrowseEntry>> {
        let taxonomy = taxonomy.trim();
        if taxonomy.is_empty() {
            return Err(MemoryError::InvalidInput(
                "browse requires a taxonomy branch".into(),
            ));
        }
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();
        let mut stmt = conn.prepare(
            "SELECT id, title, taxonomy FROM memories
             WHERE scope = ?1 AND deleted_at IS NULL AND (taxonomy = ?2 OR taxonomy LIKE ?3)
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(
            params![scope_key, taxonomy, format!("{taxonomy}.%")],
            |row| {
                Ok(BrowseEntry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    taxonomy: row.get(2)?,
                })
            },
        )?;
        let mut entries = Vec::new();
        let mut used = 0usize;
        for row in rows {
            let entry = row?;
            if let Some(budget) = token_budget {
                let cost = (entry.title.len() + entry.taxonomy.len()) / CHARS_PER_TOKEN + 1;
                if used + cost > budget {
                    break;
                }
                used += cost;
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    // ---- revisions ------------------------------------------------------------------------

    pub async fn history(&self, scope: &Scope, id: &str) -> Result<Vec<MemoryRevision>> {
        let conn = self.conn.lock().await;
        // Confirm the memory belongs to this scope before exposing history.
        if load_memory(&conn, &scope.as_key(), id)?.is_none() {
            let any: Option<String> = conn
                .query_row(
                    "SELECT id FROM memories WHERE scope = ?1 AND id = ?2",
                    params![scope.as_key(), id],
                    |row| row.get(0),
                )
                .optional()?;
            if any.is_none() {
                return Err(MemoryError::NotFound(id.to_string()));
            }
        }
        let mut stmt = conn.prepare(
            "SELECT memory_id, version, title, content, author, created_at
             FROM memory_revisions WHERE memory_id = ?1 ORDER BY version DESC",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            let created: String = row.get(5)?;
            Ok(MemoryRevision {
                memory_id: row.get(0)?,
                version: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                author: row.get(4)?,
                created_at: parse_ts(&created),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Copy an old revision forward as a new version (never rewrites history).
    pub async fn restore(
        &self,
        scope: &Scope,
        id: &str,
        version: i64,
        author: Option<&str>,
    ) -> Result<WriteResult> {
        let revision = {
            let conn = self.conn.lock().await;
            conn.query_row(
                "SELECT title, content FROM memory_revisions WHERE memory_id = ?1 AND version = ?2",
                params![id, version],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| MemoryError::NotFound(format!("version {version} of {id}")))?
        };
        self.update(
            scope,
            id,
            None,
            Some(&revision.0),
            Some(&revision.1),
            None,
            None,
            author,
        )
        .await
    }

    // ---- taxonomy template ----------------------------------------------------------------

    pub async fn taxonomy_template(&self, scope: &Scope) -> Result<Vec<TaxonomyBranch>> {
        let conn = self.conn.lock().await;
        let stored: Option<String> = conn
            .query_row(
                "SELECT branches FROM memory_taxonomy_template WHERE scope = ?1",
                params![scope.as_key()],
                |row| row.get(0),
            )
            .optional()?;
        match stored {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(default_template(scope)),
        }
    }

    pub async fn set_taxonomy_template(
        &self,
        scope: &Scope,
        branches: &[TaxonomyBranch],
    ) -> Result<()> {
        let json = serde_json::to_string(branches)?;
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO memory_taxonomy_template (scope, branches, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(scope) DO UPDATE SET branches = excluded.branches, updated_at = excluded.updated_at",
            params![scope.as_key(), json, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    // ---- graph & health -------------------------------------------------------------------

    pub async fn edge_health(&self, scope: &Scope) -> Result<EdgeHealth> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_edges WHERE scope = ?1",
            params![scope_key],
            |row| row.get(0),
        )?;
        let resolved: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_edges WHERE scope = ?1 AND dst_id IS NOT NULL",
            params![scope_key],
            |row| row.get(0),
        )?;

        let mut stmt = conn.prepare(
            "SELECT raw_target, COUNT(*) FROM memory_edges
             WHERE scope = ?1 AND dst_id IS NULL
             GROUP BY raw_target ORDER BY COUNT(*) DESC LIMIT 10",
        )?;
        let rows = stmt.query_map(params![scope_key], |row| {
            Ok(DanglingTarget {
                raw_target: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })?;
        let mut top_dangling = Vec::new();
        for row in rows {
            top_dangling.push(row?);
        }

        Ok(EdgeHealth {
            total: total as usize,
            resolved: resolved as usize,
            dangling: (total - resolved) as usize,
            top_dangling,
        })
    }

    /// Propose pairs of memories with strong title-token overlap that have no edge yet.
    pub async fn suggest_links(&self, scope: &Scope, limit: usize) -> Result<Vec<SuggestedLink>> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();

        let mut stmt = conn.prepare(
            "SELECT id, title FROM memories WHERE scope = ?1 AND deleted_at IS NULL ORDER BY updated_at DESC LIMIT 500",
        )?;
        let rows = stmt.query_map(params![scope_key.clone()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut items: Vec<(String, String)> = Vec::new();
        for row in rows {
            items.push(row?);
        }

        let mut suggestions: Vec<SuggestedLink> = Vec::new();
        for (i, (a_id, a_title)) in items.iter().enumerate() {
            for (b_id, b_title) in items.iter().skip(i + 1) {
                let score = title_overlap(a_title, b_title);
                if score < 0.5 {
                    continue;
                }
                let linked: Option<String> = conn
                    .query_row(
                        "SELECT id FROM memory_edges
                         WHERE scope = ?1 AND ((src_id = ?2 AND dst_id = ?3) OR (src_id = ?3 AND dst_id = ?2)) LIMIT 1",
                        params![scope_key, a_id, b_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if linked.is_some() {
                    continue;
                }
                suggestions.push(SuggestedLink {
                    src_id: a_id.clone(),
                    src_title: a_title.clone(),
                    dst_id: b_id.clone(),
                    dst_title: b_title.clone(),
                    overlap_score: score,
                });
            }
        }
        suggestions.sort_by(|a, b| {
            b.overlap_score
                .partial_cmp(&a.overlap_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions.truncate(limit);
        Ok(suggestions)
    }

    /// Link graph: corpus-wide (optionally taxonomy-filtered) or an ego graph around `center`.
    pub async fn graph(
        &self,
        scope: &Scope,
        center: Option<&str>,
        taxonomy: Option<&str>,
        depth: usize,
    ) -> Result<(Vec<GraphNode>, Vec<GraphEdge>)> {
        let conn = self.conn.lock().await;
        let scope_key = scope.as_key();

        let mut nodes: HashMap<String, GraphNode> = HashMap::new();
        let mut edges: Vec<GraphEdge> = Vec::new();

        match center {
            Some(center_id) => {
                load_graph_node(&conn, &scope_key, &mut nodes, center_id)?;
                let mut frontier = vec![center_id.to_string()];
                let mut visited: std::collections::HashSet<String> = [center_id.to_string()].into();
                let max_depth = depth.clamp(1, 4);
                for _ in 0..max_depth {
                    let mut next = Vec::new();
                    for id in &frontier {
                        let mut stmt = conn.prepare(
                            "SELECT src_id, dst_id, rel, raw_target FROM memory_edges
                             WHERE scope = ?1 AND (src_id = ?2 OR dst_id = ?2)",
                        )?;
                        let rows = stmt.query_map(params![scope_key, id], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, String>(3)?,
                            ))
                        })?;
                        for row in rows {
                            let (src, dst, rel, raw) = row?;
                            let rel = rel_from_str(&rel);
                            let other = if &src == id {
                                dst.clone()
                            } else {
                                Some(src.clone())
                            };
                            match other {
                                Some(other_id) => {
                                    load_graph_node(&conn, &scope_key, &mut nodes, &other_id)?;
                                    edges.push(GraphEdge {
                                        src_id: src.clone(),
                                        dst_id: other_id.clone(),
                                        rel,
                                    });
                                    if visited.insert(other_id.clone()) {
                                        next.push(other_id);
                                    }
                                }
                                None => {
                                    let ghost = format!("dangling:{}", normalize_target(&raw));
                                    nodes.entry(ghost.clone()).or_insert(GraphNode {
                                        id: ghost.clone(),
                                        title: raw.clone(),
                                        taxonomy: String::new(),
                                        authority: 0,
                                        superseded: false,
                                        dangling: true,
                                    });
                                    edges.push(GraphEdge {
                                        src_id: src.clone(),
                                        dst_id: ghost,
                                        rel,
                                    });
                                }
                            }
                        }
                    }
                    frontier = next;
                    if frontier.is_empty() {
                        break;
                    }
                }
            }
            None => {
                let (node_sql, node_args): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match taxonomy {
                    Some(branch) if !branch.trim().is_empty() => (
                        "SELECT id FROM memories WHERE scope = ?1 AND deleted_at IS NULL AND (taxonomy = ?2 OR taxonomy LIKE ?3)",
                        vec![
                            Box::new(scope_key.clone()),
                            Box::new(branch.to_string()),
                            Box::new(format!("{branch}.%")),
                        ],
                    ),
                    _ => (
                        "SELECT id FROM memories WHERE scope = ?1 AND deleted_at IS NULL",
                        vec![Box::new(scope_key.clone())],
                    ),
                };
                let mut ids = Vec::new();
                {
                    let mut stmt = conn.prepare(node_sql)?;
                    let refs: Vec<&dyn rusqlite::ToSql> =
                        node_args.iter().map(|b| b.as_ref()).collect();
                    let rows = stmt.query_map(refs.as_slice(), |row| row.get::<_, String>(0))?;
                    for row in rows {
                        ids.push(row?);
                    }
                }
                for id in &ids {
                    load_graph_node(&conn, &scope_key, &mut nodes, id)?;
                }
                let mut stmt = conn.prepare(
                    "SELECT src_id, dst_id, rel, raw_target FROM memory_edges WHERE scope = ?1",
                )?;
                let rows = stmt.query_map(params![scope_key], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?;
                for row in rows {
                    let (src, dst, rel, raw) = row?;
                    if !nodes.contains_key(&src) {
                        continue;
                    }
                    let rel = rel_from_str(&rel);
                    match dst {
                        Some(dst_id) if nodes.contains_key(&dst_id) => edges.push(GraphEdge {
                            src_id: src,
                            dst_id,
                            rel,
                        }),
                        _ => {
                            let ghost = format!("dangling:{}", normalize_target(&raw));
                            nodes.entry(ghost.clone()).or_insert(GraphNode {
                                id: ghost.clone(),
                                title: raw,
                                taxonomy: String::new(),
                                authority: 0,
                                superseded: false,
                                dangling: true,
                            });
                            edges.push(GraphEdge {
                                src_id: src,
                                dst_id: ghost,
                                rel,
                            });
                        }
                    }
                }
            }
        }

        let mut node_list: Vec<GraphNode> = nodes.into_values().collect();
        node_list.sort_by(|a, b| a.id.cmp(&b.id));
        edges.sort_by(|a, b| (&a.src_id, &a.dst_id).cmp(&(&b.src_id, &b.dst_id)));
        Ok((node_list, edges))
    }
}

// ---- schema -------------------------------------------------------------------------------

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS memories (
            id          TEXT PRIMARY KEY,
            scope       TEXT NOT NULL,
            taxonomy    TEXT NOT NULL,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL,
            version     INTEGER NOT NULL DEFAULT 1,
            author      TEXT,
            source      TEXT NOT NULL DEFAULT 'tool',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            deleted_at  TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_scope_title
            ON memories(scope, title COLLATE NOCASE) WHERE deleted_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_memories_scope_taxonomy
            ON memories(scope, taxonomy);

        CREATE TABLE IF NOT EXISTS memory_revisions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id   TEXT NOT NULL,
            version     INTEGER NOT NULL,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL,
            author      TEXT,
            created_at  TEXT NOT NULL,
            UNIQUE(memory_id, version)
        );
        CREATE INDEX IF NOT EXISTS idx_memory_revisions_memory
            ON memory_revisions(memory_id, version DESC);

        CREATE TABLE IF NOT EXISTS memory_edges (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            scope             TEXT NOT NULL,
            src_id            TEXT NOT NULL,
            dst_id            TEXT,
            raw_target        TEXT NOT NULL,
            normalized_target TEXT NOT NULL,
            rel               TEXT NOT NULL,
            UNIQUE(scope, src_id, rel, normalized_target)
        );
        CREATE INDEX IF NOT EXISTS idx_memory_edges_src ON memory_edges(scope, src_id);
        CREATE INDEX IF NOT EXISTS idx_memory_edges_dst ON memory_edges(scope, dst_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            title, content, content='memories', content_rowid='rowid'
        );

        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, title, content)
            VALUES (new.rowid, new.title, new.content);
        END;
        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, title, content)
            VALUES ('delete', old.rowid, old.title, old.content);
        END;
        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, title, content)
            VALUES ('delete', old.rowid, old.title, old.content);
            INSERT INTO memories_fts(rowid, title, content)
            VALUES (new.rowid, new.title, new.content);
        END;

        CREATE TABLE IF NOT EXISTS memory_taxonomy_template (
            scope       TEXT PRIMARY KEY,
            branches    TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );
        ",
    )?;

    // Lightweight migration: add `importance` column if it doesn't exist (existing DBs).
    let has_importance: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('memories') WHERE name = 'importance')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !has_importance {
        conn.execute_batch(
            "ALTER TABLE memories ADD COLUMN importance INTEGER NOT NULL DEFAULT 3;",
        )?;
    }

    Ok(())
}

// ---- helpers ------------------------------------------------------------------------------

fn validate_title(title: &str) -> Result<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(MemoryError::InvalidInput("title must not be empty".into()));
    }
    if trimmed.len() > 200 {
        return Err(MemoryError::InvalidInput("title too long (max 200)".into()));
    }
    if trimmed.contains('\n') {
        return Err(MemoryError::InvalidInput(
            "title must be a single line".into(),
        ));
    }
    Ok(())
}

fn validate_taxonomy(taxonomy: &str) -> Result<()> {
    let trimmed = taxonomy.trim();
    if trimmed.is_empty() {
        return Err(MemoryError::InvalidInput(
            "taxonomy must not be empty".into(),
        ));
    }
    for segment in trimmed.split('.') {
        if segment.is_empty() {
            return Err(MemoryError::InvalidInput(format!(
                "taxonomy '{taxonomy}' has an empty segment"
            )));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(MemoryError::InvalidInput(format!(
                "taxonomy segment '{segment}' may only contain letters, digits, '-' and '_'"
            )));
        }
    }
    Ok(())
}

fn split_taxonomy(path: &str) -> Vec<String> {
    path.split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn starts_with(path: &[String], prefix: &[String]) -> bool {
    prefix.len() <= path.len() && path[..prefix.len()] == *prefix
}

fn title_overlap(a: &str, b: &str) -> f64 {
    let toks = |s: &str| -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 2)
            .map(str::to_string)
            .collect()
    };
    let ta = toks(a);
    let tb = toks(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    inter / union
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
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
        created_at: parse_ts(&created),
        updated_at: parse_ts(&updated),
        deleted_at: deleted.map(|d| parse_ts(&d)),
    })
}

fn load_memory(conn: &Connection, scope: &str, id: &str) -> Result<Option<Memory>> {
    conn.query_row(
        "SELECT id, scope, taxonomy, title, content, version, author, source, importance, created_at, updated_at, deleted_at
         FROM memories WHERE scope = ?1 AND id = ?2 AND deleted_at IS NULL",
        params![scope, id],
        row_to_memory,
    )
    .optional()
    .map_err(Into::into)
}

fn insert_revision(
    conn: &Connection,
    memory_id: &str,
    version: i64,
    title: &str,
    content: &str,
    author: Option<&str>,
    now: DateTime<Utc>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO memory_revisions (memory_id, version, title, content, author, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![memory_id, version, title, content, author, now.to_rfc3339()],
    )?;
    Ok(())
}

fn sync_edges(
    conn: &Connection,
    scope: &str,
    src_id: &str,
    content: &str,
    _now: DateTime<Utc>,
) -> Result<Vec<EdgeStatus>> {
    conn.execute(
        "DELETE FROM memory_edges WHERE scope = ?1 AND src_id = ?2",
        params![scope, src_id],
    )?;

    let parsed = parse_links(content);
    let mut statuses = Vec::new();
    for link in parsed {
        let dst_id: Option<String> = conn
            .query_row(
                "SELECT id FROM memories WHERE scope = ?1 AND title = ?2 COLLATE NOCASE AND deleted_at IS NULL",
                params![scope, link.raw_target],
                |row| row.get(0),
            )
            .optional()?;
        conn.execute(
            "INSERT OR IGNORE INTO memory_edges
                (scope, src_id, dst_id, raw_target, normalized_target, rel)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                scope,
                src_id,
                dst_id,
                link.raw_target,
                link.normalized_target,
                link.rel.as_str(),
            ],
        )?;
        statuses.push(EdgeStatus {
            raw_target: link.raw_target,
            rel: link.rel,
            resolved: dst_id.is_some(),
            dst_id,
        });
    }
    Ok(statuses)
}

fn resolve_dangling(
    conn: &Connection,
    scope: &str,
    normalized_title: &str,
    dst_id: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE memory_edges SET dst_id = ?1
         WHERE scope = ?2 AND dst_id IS NULL AND normalized_target = ?3 AND src_id != ?1",
        params![dst_id, scope, normalized_title],
    )?;
    Ok(())
}

fn load_graph_node(
    conn: &Connection,
    scope: &str,
    nodes: &mut HashMap<String, GraphNode>,
    id: &str,
) -> Result<()> {
    if nodes.contains_key(id) {
        return Ok(());
    }
    if let Some(memory) = load_memory(conn, scope, id)? {
        let authority: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_edges WHERE scope = ?1 AND dst_id = ?2 AND rel IN ('relates','depends-on')",
            params![scope, id],
            |row| row.get(0),
        )?;
        let superseded: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_edges WHERE scope = ?1 AND dst_id = ?2 AND rel = 'supersedes')",
            params![scope, id],
            |row| row.get(0),
        )?;
        nodes.insert(
            id.to_string(),
            GraphNode {
                id: memory.id,
                title: memory.title,
                taxonomy: memory.taxonomy,
                authority: authority as usize,
                superseded: superseded != 0,
                dangling: false,
            },
        );
    }
    Ok(())
}

fn rel_from_str(value: &str) -> LinkRelation {
    LinkRelation::parse(value).unwrap_or(LinkRelation::Relates)
}

pub(crate) fn parse_ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_default()
}

/// Compute a recency boost: full `RECENCY_MAX_BONUS` for memories within `RECENCY_FRESH_DAYS`,
/// linearly decaying to 0 at `RECENCY_WINDOW_DAYS`, and 0 thereafter.
fn recency_boost(created_at: &str, now: DateTime<Utc>) -> f64 {
    let created = match DateTime::parse_from_rfc3339(created_at) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return 0.0,
    };
    let age_days = (now - created).num_days().max(0);
    if age_days <= RECENCY_FRESH_DAYS {
        RECENCY_MAX_BONUS
    } else if age_days >= RECENCY_WINDOW_DAYS {
        0.0
    } else {
        let remaining = (RECENCY_WINDOW_DAYS - age_days) as f64;
        let window = (RECENCY_WINDOW_DAYS - RECENCY_FRESH_DAYS) as f64;
        RECENCY_MAX_BONUS * (remaining / window)
    }
}

/// Build an FTS5 MATCH expression that is safe for arbitrary user input by quoting each term.
fn build_match_expr(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn apply_token_budget(hits: Vec<SearchHit>, budget: Option<usize>) -> (Vec<SearchHit>, usize) {
    match budget {
        None => {
            let omitted = 0;
            (hits, omitted)
        }
        Some(budget) => {
            let mut used = 0usize;
            let mut kept = Vec::new();
            let total = hits.len();
            for hit in hits {
                let cost = (hit.content.len() + hit.title.len()) / CHARS_PER_TOKEN + 1;
                if used + cost > budget {
                    break;
                }
                used += cost;
                kept.push(hit);
            }
            let omitted = total - kept.len();
            (kept, omitted)
        }
    }
}

fn default_template(scope: &Scope) -> Vec<TaxonomyBranch> {
    let kind = if scope.is_global() { "user" } else { "project" };
    vec![
        TaxonomyBranch {
            path: "overview".into(),
            description: format!("High-level facts about this {kind} and its purpose."),
        },
        TaxonomyBranch {
            path: "architecture".into(),
            description: "Structural decisions: modules, boundaries, data flow.".into(),
        },
        TaxonomyBranch {
            path: "conventions".into(),
            description: "Coding standards, naming, formatting, review norms.".into(),
        },
        TaxonomyBranch {
            path: "commands".into(),
            description: "Build, test, lint, and run commands that work here.".into(),
        },
        TaxonomyBranch {
            path: "gotchas".into(),
            description: "Known pitfalls, surprising behavior, and how to avoid them.".into(),
        },
        TaxonomyBranch {
            path: "decisions".into(),
            description: "Accepted trade-offs and why alternatives were rejected.".into(),
        },
    ]
}
