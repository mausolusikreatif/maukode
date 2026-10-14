//! Memory consolidation: detect and reconcile near-duplicate memories using an LLM.
//!
//! After each reflection cycle (or on explicit trigger), the consolidation engine:
//!
//! 1. Asks the [`MemoryStore`] for duplicate candidate pairs via [`find_duplicates`].
//! 2. Batches pairs and sends them to the LLM with a reconciliation prompt.
//! 3. Parses the LLM's decisions (merge / supersede / keep-separate).
//! 4. Applies each decision via [`MemoryStore::apply_consolidation`].
//! 5. If enabled, synthesises higher-level abstraction memories from the corpus (Tier 4).
//!
//! Consolidation is opt-in via [`ConsolidationConfig`] and never fails the turn.

use std::sync::Arc;

use maukode_llm::{LLMClient, LLMRequest};
use maukode_memory::{ConsolidationAction, ConsolidationSuggestion, DuplicatePair, MemoryContext};
pub use maukode_memory::ConsolidationReport;
use maukode_protocol::Message;
use serde::Deserialize;
use tracing::debug;

/// Configuration for the consolidation engine.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ConsolidationConfig {
    /// Whether consolidation runs after reflection.
    #[serde(default)]
    pub enabled: bool,
    /// Minimum number of live memories before consolidation is attempted.
    #[serde(default = "default_min_memories")]
    pub min_memories: usize,
    /// Jaccard similarity threshold for considering two memories duplicates (0.0–1.0).
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// Maximum number of pairs to send to the LLM in a single batch.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Whether to synthesise higher-level abstractions after merging (Tier 4).
    #[serde(default)]
    pub abstraction: bool,
    /// Minimum number of live memories (post-merge) before abstraction synthesis runs.
    #[serde(default = "default_abstraction_min")]
    pub abstraction_min_memories: usize,
}

fn default_min_memories() -> usize {
    20
}

fn default_threshold() -> f64 {
    0.5
}

fn default_batch_size() -> usize {
    10
}

fn default_abstraction_min() -> usize {
    30
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_memories: default_min_memories(),
            threshold: default_threshold(),
            batch_size: default_batch_size(),
            abstraction: false,
            abstraction_min_memories: default_abstraction_min(),
        }
    }
}

/// The system prompt sent to the LLM during consolidation.
const CONSOLIDATION_SYSTEM_PROMPT: &str = "\
You are a memory consolidation engine for a coding agent. You receive pairs of \
memories that have high textual overlap. For each pair, decide whether to merge, \
supersede, or keep them separate.\n\n\
Output STRICTLY in JSON format:\n\
\n\
[\n  {\n    \"index\": 0,\n    \"action\": \"merge\",\n    \"merged_title\": \"...\",\n    \"merged_content\": \"...\",\n    \"merged_importance\": 3,\n    \"reason\": \"...\"\n  },\n  {\n    \"index\": 1,\n    \"action\": \"supersede\",\n    \"reason\": \"...\"\n  },\n  {\n    \"index\": 2,\n    \"action\": \"keep_separate\",\n    \"reason\": \"...\"\n  }\n]\n\
\n\
Rules:\n\
- \"index\" refers to the 0-based position of the pair in the input.\n\
- For \"merge\": provide merged_title, merged_content (combined & deduplicated), \
and merged_importance (1-5, take the max of the two).\n\
- For \"supersede\": the newer memory supersedes the older one.\n\
- For \"keep_separate\": explain why they are distinct.\n\
- Be conservative: only merge when the memories clearly describe the same fact.\n\
- When in doubt, prefer \"keep_separate\".\n";

/// Wire format for the LLM's JSON response.
#[derive(Debug, Clone, Deserialize)]
struct LlmDecision {
    index: usize,
    action: String,
    #[serde(default)]
    merged_title: Option<String>,
    #[serde(default)]
    merged_content: Option<String>,
    #[serde(default)]
    merged_importance: Option<i64>,
    #[serde(default)]
    reason: Option<String>,
}

/// System prompt for the abstraction synthesis phase (Tier 4).
const ABSTRACTION_SYSTEM_PROMPT: &str = "\
You are a memory abstraction engine for a coding agent. You receive a list of \
individual memories grouped under a taxonomy branch. Your task is to identify \
recurring patterns, cross-cutting themes, or higher-level principles that emerge \
from multiple memories.\n\n\
Output STRICTLY in JSON format — an array of abstractions:\n\
\n\
[\n  {\n    \"title\": \"short-kebab-case-id\",\n    \"content\": \"A concise principle or pattern that generalises the source memories.\",\n    \"importance\": 4,\n    \"source_titles\": [\"Title 1\", \"Title 2\"]\n  }\n]\n\
\n\
Rules:\n\
- Only create an abstraction when at least 3 memories contribute to it.\n\
- The title must be a short, unique kebab-case identifier (not already in the input).\n\
- The content should be a reusable principle, not a restatement of any single memory.\n\
- source_titles must list the titles of the memories that informed the abstraction.\n\
- importance should be 1-5 (default 4 for abstractions, since they are synthesised insights).\n\
- If no meaningful abstractions can be drawn, return an empty array [].\n\
- Do NOT duplicate existing memory titles.\n";

/// Wire format for an LLM-generated abstraction.
#[derive(Debug, Clone, Deserialize)]
struct LlmAbstraction {
    title: String,
    content: String,
    #[serde(default = "default_abstraction_importance")]
    importance: i64,
    #[serde(default)]
    source_titles: Vec<String>,
}

fn default_abstraction_importance() -> i64 {
    4
}

/// The consolidation engine.
pub struct ConsolidationEngine<'a> {
    llm: &'a Arc<dyn LLMClient>,
    model: &'a str,
    context: &'a MemoryContext,
    config: ConsolidationConfig,
}

impl<'a> ConsolidationEngine<'a> {
    pub fn new(
        llm: &'a Arc<dyn LLMClient>,
        model: &'a str,
        context: &'a MemoryContext,
        config: ConsolidationConfig,
    ) -> Self {
        Self {
            llm,
            model,
            context,
            config,
        }
    }

    /// Run the consolidation loop. Returns a report of actions taken.
    /// Never returns an error — all failures are logged and swallowed.
    pub async fn run(&self) -> ConsolidationReport {
        if !self.config.enabled {
            return ConsolidationReport::default();
        }

        let store = &self.context.store;
        let scope = &self.context.scope;

        let live_count = match store.count_live(scope).await {
            Ok(n) => n,
            Err(e) => {
                debug!("Consolidation skipped: failed to count memories: {e}");
                return ConsolidationReport::default();
            }
        };

        if live_count < self.config.min_memories {
            debug!(
                "Consolidation skipped: only {live_count} live memories (min {})",
                self.config.min_memories
            );
            return ConsolidationReport::default();
        }

        let pairs = match store.find_duplicates(scope, self.config.threshold).await {
            Ok(p) => p,
            Err(e) => {
                debug!("Consolidation skipped: failed to find duplicates: {e}");
                return ConsolidationReport::default();
            }
        };

        if pairs.is_empty() {
            debug!("Consolidation: no duplicate pairs found");
            // Still run abstraction if enabled — it doesn't require duplicate pairs.
            let mut report = ConsolidationReport::default();
            if self.config.abstraction {
                self.run_abstraction(&mut report).await;
            }
            return report;
        }

        let mut report = ConsolidationReport {
            pairs_examined: pairs.len(),
            ..Default::default()
        };

        // Process in batches.
        for batch in pairs.chunks(self.config.batch_size) {
            let suggestions = match self.reconcile_batch(batch).await {
                Ok(s) => s,
                Err(e) => {
                    debug!("Consolidation batch failed: {e}");
                    report.skipped += batch.len();
                    continue;
                }
            };

            for suggestion in &suggestions {
                if let Err(e) = store
                    .apply_consolidation(scope, suggestion, self.context.author.as_deref())
                    .await
                {
                    debug!("Consolidation apply failed: {e}");
                    report.skipped += 1;
                    continue;
                }
                match &suggestion.action {
                    ConsolidationAction::Merge { .. } => report.merged += 1,
                    ConsolidationAction::Supersede { .. } => report.superseded += 1,
                    ConsolidationAction::KeepSeparate { .. } => report.kept_separate += 1,
                }
            }
        }

        // Tier 4: synthesise higher-level abstractions from the memory corpus.
        if self.config.abstraction {
            self.run_abstraction(&mut report).await;
        }

        report
    }

    /// Send a batch of duplicate pairs to the LLM and parse the decisions.
    async fn reconcile_batch(
        &self,
        pairs: &[DuplicatePair],
    ) -> anyhow::Result<Vec<ConsolidationSuggestion>> {
        let prompt = format_batch_prompt(pairs);

        let request = LLMRequest::new(self.model.to_string(), vec![Message::user(prompt)])
            .with_system_prompt(CONSOLIDATION_SYSTEM_PROMPT)
            .with_max_tokens(2048);

        let mut stream = self.llm.stream(request).await?;
        use futures::StreamExt;
        let mut full_text = String::new();
        while let Some(event) = stream.next().await {
            if let Ok(maukode_protocol::LLMEvent::TextDelta { text }) = event {
                full_text.push_str(&text);
            }
        }

        // Extract JSON from the response (it may be wrapped in ```json ... ```).
        let json_text = extract_json(&full_text);
        let decisions: Vec<LlmDecision> = serde_json::from_str(json_text)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM consolidation response: {e}"))?;

        let mut suggestions = Vec::new();
        for decision in decisions {
            if decision.index >= pairs.len() {
                continue;
            }
            let pair = &pairs[decision.index];
            let action = match decision.action.as_str() {
                "merge" => {
                    let title = decision
                        .merged_title
                        .as_deref()
                        .unwrap_or(&pair.older.title)
                        .to_string();
                    let content = decision
                        .merged_content
                        .as_deref()
                        .unwrap_or(&pair.older.content)
                        .to_string();
                    let importance = decision
                        .merged_importance
                        .unwrap_or(pair.older.importance.max(pair.newer.importance))
                        .clamp(1, 5);
                    ConsolidationAction::Merge {
                        merged_title: title,
                        merged_content: content,
                        merged_importance: importance,
                    }
                }
                "supersede" => ConsolidationAction::Supersede {
                    reason: decision.reason.unwrap_or_default(),
                },
                _ => ConsolidationAction::KeepSeparate {
                    reason: decision.reason.unwrap_or_default(),
                },
            };
            suggestions.push(ConsolidationSuggestion {
                pair: pair.clone(),
                action,
            });
        }

        // Any pairs the LLM didn't address are treated as keep-separate.
        for pair in pairs.iter() {
            if !suggestions
                .iter()
                .any(|s| s.pair.older.id == pair.older.id && s.pair.newer.id == pair.newer.id)
            {
                suggestions.push(ConsolidationSuggestion {
                    pair: pair.clone(),
                    action: ConsolidationAction::KeepSeparate {
                        reason: "Not addressed by LLM".into(),
                    },
                });
            }
        }

        Ok(suggestions)
    }

    /// Tier 4: Synthesise higher-level abstractions from the live memory corpus.
    ///
    /// Loads all live memories, groups them by taxonomy, and asks the LLM to identify
    /// cross-cutting patterns that deserve a new "abstraction" memory. Each abstraction
    /// links back to its source memories via `[[Title]]` links.
    async fn run_abstraction(&self, report: &mut ConsolidationReport) {
        let store = &self.context.store;
        let scope = &self.context.scope;

        // Only run if enough memories exist.
        let live_count = match store.count_live(scope).await {
            Ok(n) => n,
            Err(e) => {
                debug!("Abstraction skipped: failed to count live memories: {e}");
                return;
            }
        };
        if live_count < self.config.abstraction_min_memories {
            debug!(
                "Abstraction skipped: only {} live memories (need {})",
                live_count, self.config.abstraction_min_memories
            );
            return;
        }

        let memories = match store.list_live(scope).await {
            Ok(m) => m,
            Err(e) => {
                debug!("Abstraction skipped: failed to list live memories: {e}");
                return;
            }
        };

        // Collect existing titles to avoid duplicate abstractions.
        let existing_titles: std::collections::HashSet<String> =
            memories.iter().map(|m| m.title.to_lowercase()).collect();

        // Group memories by top-level taxonomy branch.
        let mut groups: std::collections::BTreeMap<String, Vec<&maukode_memory::Memory>> =
            std::collections::BTreeMap::new();
        for mem in &memories {
            let branch = mem
                .taxonomy
                .split('.')
                .next()
                .unwrap_or("overview")
                .to_string();
            groups.entry(branch).or_default().push(mem);
        }

        for (branch, group) in &groups {
            // Skip branches with too few memories to abstract from.
            if group.len() < 3 {
                continue;
            }

            let prompt = format_abstraction_prompt(branch, group);
            let request = LLMRequest::new(self.model.to_string(), vec![Message::user(prompt)])
                .with_system_prompt(ABSTRACTION_SYSTEM_PROMPT)
                .with_max_tokens(2048);

            let stream = match self.llm.stream(request).await {
                Ok(s) => s,
                Err(e) => {
                    debug!("Abstraction LLM stream failed for branch '{branch}': {e}");
                    continue;
                }
            };

            use futures::StreamExt;
            let mut full_text = String::new();
            let mut s = stream;
            while let Some(event) = s.next().await {
                if let Ok(maukode_protocol::LLMEvent::TextDelta { text }) = event {
                    full_text.push_str(&text);
                }
            }

            let json_text = extract_json(&full_text);
            let abstractions: Vec<LlmAbstraction> = match serde_json::from_str(json_text) {
                Ok(a) => a,
                Err(e) => {
                    debug!("Abstraction: failed to parse LLM response for '{branch}': {e}");
                    continue;
                }
            };

            for abstr in abstractions {
                // Skip if an abstraction with the same title already exists.
                if existing_titles.contains(&abstr.title.to_lowercase()) {
                    continue;
                }

                // Build content with back-links to source memories.
                let source_links: Vec<String> = abstr
                    .source_titles
                    .iter()
                    .map(|t| format!("- [[{t}]]"))
                    .collect();
                let content = if source_links.is_empty() {
                    abstr.content
                } else {
                    format!("{}\n\nSources:\n{}", abstr.content, source_links.join("\n"))
                };

                let taxonomy = format!("{branch}.abstraction");

                match store
                    .create(
                        scope,
                        &taxonomy,
                        &abstr.title,
                        &content,
                        abstr.importance.clamp(1, 5),
                        self.context.author.as_deref(),
                        "consolidation",
                    )
                    .await
                {
                    Ok(_) => {
                        report.abstractions_created += 1;
                        debug!(
                            "Abstraction created: '{}' (taxonomy: {taxonomy})",
                            abstr.title
                        );
                    }
                    Err(e) => {
                        debug!(
                            "Abstraction: failed to write '{}' for '{branch}': {e}",
                            abstr.title
                        );
                    }
                }
            }
        }
    }
}

/// Format the user prompt containing all duplicate pairs for LLM reconciliation.
fn format_batch_prompt(pairs: &[DuplicatePair]) -> String {
    let mut out = String::from(
        "Below are pairs of memories with high textual overlap. \
         For each pair, decide whether to merge, supersede, or keep separate.\n\n",
    );
    for (i, pair) in pairs.iter().enumerate() {
        out.push_str(&format!(
            "--- Pair {i} ---\n\
             Title similarity: {:.2}, Content similarity: {:.2}\n\n\
             OLDER (id: {}, created: {}):\n\
             Title: {}\n\
             Taxonomy: {}\n\
             Content: {}\n\n\
             NEWER (id: {}, created: {}):\n\
             Title: {}\n\
             Taxonomy: {}\n\
             Content: {}\n\n",
            pair.title_similarity,
            pair.content_similarity,
            pair.older.id,
            pair.older.created_at.format("%Y-%m-%d"),
            pair.older.title,
            pair.older.taxonomy,
            pair.older.content,
            pair.newer.id,
            pair.newer.created_at.format("%Y-%m-%d"),
            pair.newer.title,
            pair.newer.taxonomy,
            pair.newer.content,
        ));
    }
    out
}

/// Extract JSON array from a potentially markdown-wrapped LLM response.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```json") {
        if let Some(end) = trimmed.rfind("```") {
            let json_start = start + 7;
            if json_start < end {
                return trimmed[json_start..end].trim();
            }
        }
    }
    if let Some(start) = trimmed.find("```") {
        if let Some(end) = trimmed.rfind("```") {
            let json_start = start + 3;
            if json_start < end {
                return trimmed[json_start..end].trim();
            }
        }
    }
    // Find the first '[' and matching last ']'.
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if start < end {
                return &trimmed[start..=end];
            }
        }
    }
    trimmed
}

/// Format the user prompt for abstraction synthesis, listing all memories in a taxonomy branch.
fn format_abstraction_prompt(branch: &str, memories: &[&maukode_memory::Memory]) -> String {
    let mut out = format!(
        "Below are all live memories under the '{branch}' taxonomy branch. \
         Identify cross-cutting patterns or higher-level principles that emerge \
         from at least 3 of these memories.\n\n"
    );

    for (i, mem) in memories.iter().enumerate() {
        out.push_str(&format!(
            "--- Memory {i} ---\nTitle: {}\nTaxonomy: {}\nContent: {}\n\n",
            mem.title, mem.taxonomy, mem.content
        ));
    }

    out.push_str(
        "Return a JSON array of abstractions. Each abstraction must reference \
         at least 3 source memories by title. If no meaningful patterns emerge, \
         return [].\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn extract_json_plain() {
        assert_eq!(extract_json(r#"[]"#), "[]");
        assert_eq!(extract_json(r#"  [{"index":0}]  "#), r#"[{"index":0}]"#);
    }

    #[test]
    fn extract_json_markdown() {
        let input = "```json\n[]\n```";
        assert_eq!(extract_json(input), "[]");
    }

    #[test]
    fn extract_json_plain_code_fence() {
        let input = r#"```
[{"index":0}]
```"#;
        assert_eq!(extract_json(input), r#"[{"index":0}]"#);
    }

    #[test]
    fn format_batch_prompt_contains_pair_info() {
        let pair = DuplicatePair {
            older: maukode_memory::Memory {
                id: "a".into(),
                scope: "global".into(),
                taxonomy: "gotchas".into(),
                title: "Test Memory".into(),
                content: "Some content here".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 3,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
            },
            newer: maukode_memory::Memory {
                id: "b".into(),
                scope: "global".into(),
                taxonomy: "gotchas".into(),
                title: "Test Memory v2".into(),
                content: "Some content here too".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 4,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
            },
            title_similarity: 0.8,
            content_similarity: 0.6,
        };
        let prompt = format_batch_prompt(&[pair]);
        assert!(prompt.contains("Pair 0"));
        assert!(prompt.contains("Test Memory"));
        assert!(prompt.contains("Test Memory v2"));
    }

    #[test]
    fn format_abstraction_prompt_contains_memories() {
        let now = Utc::now();
        let memories = vec![
            maukode_memory::Memory {
                id: "1".into(),
                scope: "global".into(),
                taxonomy: "gotchas.sqlite".into(),
                title: "SQLite WAL mode".into(),
                content: "Enable WAL mode for concurrent reads".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 3,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
            maukode_memory::Memory {
                id: "2".into(),
                scope: "global".into(),
                taxonomy: "gotchas.sqlite".into(),
                title: "SQLite busy timeout".into(),
                content: "Set busy_timeout to avoid lock errors".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 3,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
            maukode_memory::Memory {
                id: "3".into(),
                scope: "global".into(),
                taxonomy: "gotchas.sqlite".into(),
                title: "SQLite vacuum".into(),
                content: "Run VACUUM periodically to reclaim space".into(),
                version: 1,
                author: None,
                source: "tool".into(),
                importance: 2,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
        ];
        let refs: Vec<&maukode_memory::Memory> = memories.iter().collect();
        let prompt = format_abstraction_prompt("gotchas", &refs);
        assert!(prompt.contains("SQLite WAL mode"));
        assert!(prompt.contains("SQLite busy timeout"));
        assert!(prompt.contains("gotchas"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn llm_abstraction_parses() {
        let json = r#"[
            {
                "title": "sqlite-best-practices",
                "content": "Configure SQLite for concurrency: WAL mode, busy timeout, periodic VACUUM.",
                "importance": 4,
                "source_titles": ["SQLite WAL mode", "SQLite busy timeout", "SQLite vacuum"]
            }
        ]"#;
        let abstractions: Vec<LlmAbstraction> = serde_json::from_str(json).unwrap();
        assert_eq!(abstractions.len(), 1);
        assert_eq!(abstractions[0].title, "sqlite-best-practices");
        assert_eq!(abstractions[0].importance, 4);
        assert_eq!(abstractions[0].source_titles.len(), 3);
    }

    #[test]
    fn llm_abstraction_defaults_importance() {
        let json = r#"[{"title":"test","content":"x","source_titles":[]}]"#;
        let abstractions: Vec<LlmAbstraction> = serde_json::from_str(json).unwrap();
        assert_eq!(abstractions[0].importance, 4);
    }
}
