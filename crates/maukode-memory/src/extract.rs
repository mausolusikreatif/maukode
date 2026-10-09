//! Automatic end-of-turn memory extraction.
//!
//! This runs *after* an agent turn completes and proposes candidate memories from the finished
//! conversation. Candidates are deduplicated against existing titles so the same fact is folded
//! into an update rather than duplicated. Extraction is opt-in (see `config`) and always writes
//! with `source = "auto"` so automatic entries are distinguishable from deliberate ones.

use crate::error::Result;
use crate::scope::Scope;
use crate::store::MemoryStore;
use maukode_protocol::Message;
use serde::{Deserialize, Serialize};

/// A candidate memory proposed by extraction, before it is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateMemory {
    pub title: String,
    pub taxonomy: String,
    pub content: String,
}

/// Outcome of running extraction over a turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionReport {
    pub proposed: usize,
    pub written: usize,
    pub updated: usize,
    pub skipped: usize,
}

impl ExtractionReport {
    fn count_skipped(&mut self) {
        self.skipped += 1;
    }
}

/// Write extracted candidates into the store under `scope`, deduplicating by title.
///
/// A candidate whose title already exists (case-insensitively) in the scope is skipped rather
/// than duplicated; callers that want merging should route such candidates through `update`
/// themselves with the returned existing id. Returns a per-candidate report.
pub async fn persist_candidates(
    store: &MemoryStore,
    scope: &Scope,
    candidates: &[CandidateMemory],
    author: Option<&str>,
) -> Result<ExtractionReport> {
    let mut report = ExtractionReport {
        proposed: candidates.len(),
        ..Default::default()
    };

    for candidate in candidates {
        if candidate.title.trim().is_empty() || candidate.content.trim().is_empty() {
            report.count_skipped();
            continue;
        }

        match store.by_title(scope, &candidate.title).await? {
            Some(existing) => {
                // Fold new content into the existing memory as a new revision.
                let merged = merge_content(&existing.content, &candidate.content);
                if merged == existing.content {
                    report.count_skipped();
                    continue;
                }
                store
                    .update(
                        scope,
                        &existing.id,
                        None,
                        None,
                        Some(&merged),
                        Some(existing.version),
                        author,
                    )
                    .await?;
                report.updated += 1;
            }
            None => {
                store
                    .create(
                        scope,
                        &candidate.taxonomy,
                        &candidate.title,
                        &candidate.content,
                        author,
                        "auto",
                    )
                    .await?;
                report.written += 1;
            }
        }
    }

    Ok(report)
}

/// Lightweight, LLM-free candidate derivation from a finished turn.
///
/// This is intentionally conservative: it only proposes memories from explicit, structured cues a
/// turn leaves behind (for example a captured "decision" or "convention" note). Full semantic
/// extraction is expected to be driven by the model through the `memory_write` tool; this exists
/// so a turn that ends without an explicit write can still surface obvious, durable notes.
pub fn candidates_from_messages(messages: &[Message]) -> Vec<CandidateMemory> {
    let mut candidates = Vec::new();
    for message in messages {
        let text = message.text_content();
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("[memory:") {
                if let Some(end) = rest.find(']') {
                    let payload = &rest[..end];
                    if let Some((title, content)) = payload.split_once('|') {
                        candidates.push(CandidateMemory {
                            title: title.trim().to_string(),
                            taxonomy: "extracted".to_string(),
                            content: content.trim().to_string(),
                        });
                    }
                }
            }
        }
    }
    candidates
}

fn merge_content(existing: &str, addition: &str) -> String {
    if existing.trim().is_empty() {
        return addition.to_string();
    }
    if existing.contains(addition.trim()) {
        return existing.to_string();
    }
    format!("{}\n\n{}", existing.trim_end(), addition.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_content_appends_without_duplicating() {
        assert_eq!(merge_content("a", "a"), "a");
        assert_eq!(merge_content("a", "b"), "a\n\nb");
        assert_eq!(merge_content("", "b"), "b");
    }

    #[test]
    fn candidates_are_read_from_memory_markers() {
        let messages = vec![Message::assistant_text(
            "some text\n[memory: Build command | run cargo test --workspace]\nmore",
        )];
        let candidates = candidates_from_messages(&messages);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "Build command");
        assert_eq!(candidates[0].content, "run cargo test --workspace");
    }
}
