//! `maukode-memory`: a native, embedding-free, project-scoped knowledge store.
//!
//! The store borrows the proven model from rekam (taxonomy, typed `[[wiki-links]]`, FTS5 ranking
//! with backlink authority, revisions, tombstones) and reimplements it in Rust on MauKode's
//! existing SQLite database. Retrieval is deterministic FTS5 only — no embeddings, no vector
//! store. Every read and write is filtered by a [`Scope`], the security boundary.

pub mod consolidation;
pub mod error;
pub mod extract;
pub mod links;
pub mod models;
pub mod scope;
pub mod store;
pub mod tools;

pub use consolidation::{
    ConsolidationAction, ConsolidationReport, ConsolidationSuggestion, DuplicatePair,
};
pub use error::{MemoryError, Result};
pub use extract::{persist_candidates, CandidateMemory, ExtractionReport};
pub use links::{normalize_target, parse_links, ParsedLink};
pub use models::{
    BrowseEntry, CatalogEntry, DanglingTarget, EdgeHealth, EdgeStatus, GraphEdge, GraphNode,
    LinkRelation, Memory, MemoryEdge, MemoryRevision, SearchHit, SearchResults, SuggestedLink,
    TaxonomyBranch, WriteResult,
};
pub use scope::Scope;
pub use store::MemoryStore;
pub use tools::{register_tools, taxonomy_branches, MemoryContext};

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn store() -> (MemoryStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("memory.db")).unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn create_and_search_round_trip() {
        let (store, _dir) = store();
        let scope = Scope::global();
        store
            .create(
                &scope,
                "commands",
                "Build Command",
                "Run cargo test --workspace for the full suite.",
                3,
                Some("tester"),
                "tool",
            )
            .await
            .unwrap();

        let results = store.search(&scope, "cargo", None, None).await.unwrap();
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].title, "Build Command");
        assert_eq!(results.omitted_count, 0);
    }

    #[tokio::test]
    async fn scope_isolation() {
        let (store, _dir) = store();
        let a = Scope::project("/tmp/does-not-exist-a");
        let b = Scope::project("/tmp/does-not-exist-b");
        store
            .create(&a, "overview", "Secret", "only in a", 3, None, "tool")
            .await
            .unwrap();
        let results = store.search(&b, "secret", None, None).await.unwrap();
        assert!(results.results.is_empty());
    }

    #[tokio::test]
    async fn duplicate_title_is_rejected() {
        let (store, _dir) = store();
        let scope = Scope::global();
        store
            .create(&scope, "x", "Dup", "one", 3, None, "tool")
            .await
            .unwrap();
        let err = store
            .create(&scope, "x", "dup", "two", 3, None, "tool")
            .await
            .unwrap_err();
        assert!(matches!(err, MemoryError::DuplicateTitle(_)));
    }

    #[tokio::test]
    async fn typed_links_resolve_and_supersede() {
        let (store, _dir) = store();
        let scope = Scope::global();
        store
            .create(
                &scope,
                "decisions",
                "Old Plan",
                "the first plan",
                3,
                None,
                "tool",
            )
            .await
            .unwrap();
        let write = store
            .create(
                &scope,
                "decisions",
                "New Plan",
                "replaces [[supersedes::Old Plan]] and depends on [[depends-on::Old Plan]]",
                3,
                None,
                "tool",
            )
            .await
            .unwrap();
        assert_eq!(write.links.len(), 2);
        assert!(write.links.iter().all(|l| l.resolved));

        let old = store.by_title(&scope, "Old Plan").await.unwrap().unwrap();
        let results = store.search(&scope, "plan", None, None).await.unwrap();
        let old_hit = results.results.iter().find(|h| h.id == old.id).unwrap();
        assert!(old_hit.superseded);
    }

    #[tokio::test]
    async fn dangling_link_resolves_on_later_create() {
        let (store, _dir) = store();
        let scope = Scope::global();
        store
            .create(
                &scope,
                "a",
                "Source",
                "points at [[Future Doc]]",
                3,
                None,
                "tool",
            )
            .await
            .unwrap();
        let health = store.edge_health(&scope).await.unwrap();
        assert_eq!(health.dangling, 1);

        store
            .create(&scope, "a", "Future Doc", "arrives later", 3, None, "tool")
            .await
            .unwrap();
        let health = store.edge_health(&scope).await.unwrap();
        assert_eq!(health.dangling, 0);
        assert_eq!(health.resolved, 1);
    }

    #[tokio::test]
    async fn revisions_and_restore() {
        let (store, _dir) = store();
        let scope = Scope::global();
        let created = store
            .create(&scope, "x", "Doc", "v1", 3, None, "tool")
            .await
            .unwrap();
        store
            .update(
                &scope,
                &created.memory.id,
                None,
                None,
                Some("v2"),
                None,
                Some(1),
                None,
            )
            .await
            .unwrap();
        let history = store.history(&scope, &created.memory.id).await.unwrap();
        assert_eq!(history.len(), 2);

        let restored = store
            .restore(&scope, &created.memory.id, 1, None)
            .await
            .unwrap();
        assert_eq!(restored.memory.content, "v1");
        assert_eq!(restored.memory.version, 3);
    }

    #[tokio::test]
    async fn version_conflict_is_reported() {
        let (store, _dir) = store();
        let scope = Scope::global();
        let created = store
            .create(&scope, "x", "Doc", "v1", 3, None, "tool")
            .await
            .unwrap();
        let err = store
            .update(
                &scope,
                &created.memory.id,
                None,
                None,
                Some("v2"),
                None,
                Some(99),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, MemoryError::VersionConflict { .. }));
    }

    #[tokio::test]
    async fn search_token_budget_reports_omissions() {
        let (store, _dir) = store();
        let scope = Scope::global();
        for i in 0..5 {
            store
                .create(
                    &scope,
                    "x",
                    &format!("Doc {i}"),
                    &format!("shared keyword with a lot of extra content number {i}"),
                    3,
                    None,
                    "tool",
                )
                .await
                .unwrap();
        }
        let full = store.search(&scope, "keyword", None, None).await.unwrap();
        assert_eq!(full.results.len(), 5);

        let budgeted = store
            .search(&scope, "keyword", None, Some(20))
            .await
            .unwrap();
        assert!(budgeted.results.len() < 5);
        assert_eq!(budgeted.results.len() + budgeted.omitted_count, 5);
    }

    #[tokio::test]
    async fn catalog_rolls_up_and_drills() {
        let (store, _dir) = store();
        let scope = Scope::global();
        store
            .create(&scope, "finance.accounts", "A", "a", 3, None, "tool")
            .await
            .unwrap();
        store
            .create(&scope, "finance.accounts", "B", "b", 3, None, "tool")
            .await
            .unwrap();
        store
            .create(&scope, "finance.budget", "C", "c", 3, None, "tool")
            .await
            .unwrap();

        let top = store.catalog(&scope, None, 1).await.unwrap();
        let finance = top.iter().find(|e| e.path == "finance").unwrap();
        assert_eq!(finance.count, 3);
        assert!(finance.expandable);

        let drilled = store.catalog(&scope, Some("finance"), 1).await.unwrap();
        assert!(drilled
            .iter()
            .any(|e| e.path == "finance.accounts" && e.count == 2));
    }

    #[tokio::test]
    async fn delete_is_a_tombstone() {
        let (store, _dir) = store();
        let scope = Scope::global();
        let created = store
            .create(&scope, "x", "Gone", "temporary", 3, None, "tool")
            .await
            .unwrap();
        assert!(store.delete(&scope, &created.memory.id).await.unwrap());
        assert!(store
            .get(&scope, &created.memory.id)
            .await
            .unwrap()
            .is_none());
        // History survives the tombstone.
        let history = store.history(&scope, &created.memory.id).await.unwrap();
        assert_eq!(history.len(), 1);
    }

    #[tokio::test]
    async fn extraction_dedupes_by_title() {
        let (store, _dir) = store();
        let scope = Scope::global();
        let candidates = vec![
            CandidateMemory {
                title: "Fact".into(),
                taxonomy: "x".into(),
                content: "first".into(),
                importance: None,
            },
            CandidateMemory {
                title: "Fact".into(),
                taxonomy: "x".into(),
                content: "second".into(),
                importance: None,
            },
        ];
        let report = persist_candidates(&store, &scope, &candidates, Some("auto"))
            .await
            .unwrap();
        assert_eq!(report.proposed, 2);
        assert_eq!(report.written, 1);
        assert_eq!(report.updated, 1);
        let stored = store.by_title(&scope, "Fact").await.unwrap().unwrap();
        assert!(stored.content.contains("first"));
        assert!(stored.content.contains("second"));
    }
}
