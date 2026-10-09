# Memory Architecture

This document specifies `maukode-memory`: a native, embedding-free, project-scoped
knowledge store for the MauKode coding agent. The design is adapted from the proven
model used by [rekam](https://github.com/Ucok23/rekam) (taxonomy, typed wiki-links,
FTS5 ranking, revisions, tombstones), reimplemented in Rust on MauKode's existing
SQLite store. It is **not** an MCP server and carries no external runtime dependency.

## 1. Goals and non-goals

### Goals
- Give the agent durable, retrievable project knowledge that outlives a session.
- Keep retrieval deterministic and offline: FTS5 only, **no embeddings, no vector store**.
- Preserve history: every write is revisioned; deletes are tombstones, never hard deletes.
- Enforce a hard scope boundary so memories never leak across projects.
- Expose the same behavior to explicit agent tools **and** automatic end-of-turn extraction.

### Non-goals
- No semantic/vector search.
- No general-purpose product surface (no HTTP API, no auth, no tenancy). Scope is a string key.
- No separate database. Memory tables live in the existing `maukode.db`.
- No cross-machine sync or replication in v1.

### Naming correction
The current `maukode-core/src/memory/` module (AGENTS.md / CLAUDE.md discovery) is
**rules discovery**, not a knowledge store. This spec renames it to `rules/` to end the
collision. `maukode-memory` is the new knowledge store.

## 2. Scope model

Scope is a string key resolved per session and applied to **every read and write**. It is
the security boundary.

| Tier | Key format | v1 | Notes |
|---|---|---|---|
| Project | `project:<canonical repo root>` | yes | Default. Canonicalized absolute path. |
| Global | `global` | yes | Cross-project user preferences. Explicit opt-in. |
| Session | `session:<uuid>` | no | Episodic history already lives in `messages`. |

Rules:
- Resolution order per session: explicit override, else `project:<root>`.
- A project scope never reads or matches global rows unless the caller explicitly widens scope.
- Global memories are only written when a tool call requests `scope: "global"`.
- All indexes and FTS queries are filtered by `scope` first.

This mirrors rekam's `Scope` filtering, simplified to a single string column.

## 3. Data model

New tables added to `maukode.db` via the existing `SqliteSessionStore` migration path
(`CREATE TABLE IF NOT EXISTS` + additive `ALTER`). All timestamps are RFC 3339 text, matching
current tables.

```sql
CREATE TABLE IF NOT EXISTS memories (
    id          TEXT PRIMARY KEY,          -- uuid
    scope       TEXT NOT NULL,             -- 'project:/abs/path' | 'global'
    taxonomy    TEXT NOT NULL,             -- dot-notation, e.g. work.projects.acme
    title       TEXT NOT NULL,             -- link target, unique per (scope, normalized title)
    content     TEXT NOT NULL,             -- markdown, may contain [[links]]
    version     INTEGER NOT NULL DEFAULT 1,-- optimistic concurrency
    author      TEXT,                      -- agent/model identity, self-reported
    source      TEXT NOT NULL DEFAULT 'tool', -- 'tool' | 'auto'
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT                       -- tombstone marker; NULL = live
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
    dst_id            TEXT,                      -- NULL = dangling (target not written yet)
    raw_target        TEXT NOT NULL,             -- as written, e.g. 'Q3 Launch'
    normalized_target TEXT NOT NULL,             -- lowercased/collapsed, for resolution
    rel               TEXT NOT NULL,             -- relates|depends-on|supersedes|contradicts
    UNIQUE(scope, src_id, rel, normalized_target)
);
CREATE INDEX IF NOT EXISTS idx_memory_edges_src ON memory_edges(scope, src_id);
CREATE INDEX IF NOT EXISTS idx_memory_edges_dst ON memory_edges(scope, dst_id);

-- FTS5 external-content table over live memories.
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    title, content, content='memories', content_rowid='rowid'
);

CREATE TABLE IF NOT EXISTS memory_taxonomy_template (
    scope       TEXT PRIMARY KEY,
    branches    TEXT NOT NULL,             -- JSON: [{path, description}]
    updated_at  TEXT NOT NULL
);
```

Notes:
- `deleted_at` powers tombstones. A resolved edge into a deleted memory stays bound to that
  burned `id`; it never re-homes onto a new memory that reuses the title.
- `memory_edges` uniqueness collapses duplicate `(rel, target)` pairs within one source.
- FTS is kept in sync by triggers on `memories` (implemented in `maukode-memory`), not by the
  caller, so explicit and automatic writes cannot desync it.

## 4. Taxonomy

- Dot-notation paths: `work.projects.acme`, `engineering.conventions`, `personal.health`.
- **Consistency is the rule.** A `memory_taxonomy_template` row declares the corpus's intended
  branches with a "what goes here" description each. It is injected into the agent context so
  the model files consistently instead of inventing ad-hoc paths.
- Writes validate the path shape. On an unknown branch the error response includes the current
  catalog so the model can pick an existing path or create a valid new one, matching rekam's
  behavior.
- **Catalog** aggregates live memories by path (counts only; a path with zero memories never
  appears) and rolls up to a shallow, drillable tree: top-level segments first with summed child
  counts and `expandable`, then drill in by prefix.

## 5. Typed wiki-links

Parsed at write time from `content`, stored as `memory_edges`, kept in sync on update.

- Syntax: `[[Title]]` or `[[rel::Title]]`.
- Fixed relation vocabulary: `relates` (default), `depends-on`, `supersedes`, `contradicts`.
- An unrecognized prefix degrades to a plain `relates` link on the literal bracket text
  (graceful, never rejected).
- Links inside fenced or inline code spans are **not** parsed.
- Title matching is normalized (lowercased, whitespace-collapsed, trimmed) and scoped.
- A link whose target does not exist is kept **dangling** (`dst_id IS NULL`) and resolves
  automatically once the target is created.
- Ranking effects: inbound `relates` / `depends-on` edges are **authority**;
  `supersedes` **demotes** the target; `contradicts` is neutral.

## 6. Search and retrieval

Search blends FTS5 relevance with link signals under a token budget:

```
score = fts5_rank(title, content)
      + authority_weight  * inbound_authority(id)
      - supersede_penalty * (1 if superseded else 0)
```

- `memory_search(q, taxonomy?, budget)` requires a non-empty `q`.
- Scope filter is always applied first.
- `taxonomy` scopes to a branch and its sub-branches.
- `token_budget` truncates to the top-ranked results that fit and returns `omitted_count` so the
  caller knows results were capped rather than empty.
- A negative/non-integer budget is rejected, not coerced.
- `memory_catalog(prefix?)` is discovery only; `memory_browse(taxonomy, budget)` returns
  title-level rows (id, title, taxonomy; no content) most-recently-updated first, and requires a
  branch (browsing unbound is refused).

## 7. Revisions, tombstones, and health

- Every create/update appends a `memory_revisions` row. `memory_history(id)` lists newest first.
- `memory_restore(id, version)` copies an old revision forward as a new version (no rewrites).
- `memory_update` accepts an optional `base_version` for optimistic concurrency; a mismatch is a
  conflict error, not a silent overwrite.
- `memory_delete(id)` sets `deleted_at` (tombstone) and records who/when; the title is burned.
- `memory_edge_health` reports total/resolved/dangling counts and top unresolved targets: high
  dangling means mistyped titles; near-zero total means linking is being skipped.
- `memory_suggest_links` proposes strong-text-overlap pairs that have **no** edge yet (the inverse
  of a dangling link) so the agent can connect an island.

## 8. Agent integration

### 8.1 Context assembly
`context.rs` injects, within a strict token budget:
1. the scope's taxonomy template (branches + "what goes here"),
2. top-ranked search hits relevant to the current task,
3. a reminder that islands (memories with no links) are hard to recall.

### 8.2 Tools (via the unified registry)
`memory_search`, `memory_catalog`, `memory_browse`, `memory_write`, `memory_get`,
`memory_update`, `memory_history`, `memory_restore`, `memory_delete`, `memory_graph`,
`memory_edge_health`, `memory_suggest_links`.

All are registered as `Tool` implementations alongside native and MCP tools (see §10).

### 8.3 Write policy — both explicit and automatic
- **Explicit**: the agent calls `memory_write` / `memory_update` when it decides something is
  worth remembering. This is the primary path.
- **Automatic**: at turn end, an extraction pass proposes candidate memories (durable decisions,
  conventions, gotchas, resolved problems). Proposals are:
  - deduplicated against existing titles (skip or fold into an update),
  - written with `source = 'auto'` and `author` set,
  - never written for transient/session-local content,
  - subject to the same scope, taxonomy, and revision rules as explicit writes.
  Automatic extraction is configurable and off by default until the taxonomy stabilizes.

## 9. Crate layout and boundaries

```
crates/maukode-memory/
├── src/
│   ├── lib.rs          # crate facade, re-exports, integration tests
│   ├── store.rs        # SQLite tables, migrations, triggers, FTS5 search,
│   │                   # catalog/browse, revisions, graph, edge health
│   ├── links.rs        # [[...]] parser, edge sync, dangling resolution
│   ├── models.rs       # Memory, revisions, edges, search/hit, graph types
│   ├── scope.rs        # Scope key (project:/abs/path | global)
│   ├── extract.rs      # end-of-turn automatic extraction
│   ├── error.rs        # MemoryError / Result
│   └── tools.rs        # Tool impls for the registry
```

- Depends only on `maukode-protocol`, `maukode-tools` (for the `Tool` trait), and `rusqlite`.
  No dependency on `maukode-core`.
- Uses the **same** `maukode.db` file, sharing the connection/migration discipline already in
  `store/sqlite.rs` (WAL, busy timeout, retry-on-busy).
- `maukode-core` depends on `maukode-memory`; the runner reaches memory **only through context
  assembly and registered tools**, never by touching its tables directly.
- The existing rules-discovery module is renamed `maukode-core/src/rules/`.

## 10. Relationship to the unified tool registry

The registry already exists and MCP tools already implement `Tool` and are registered through it
(`maukode-tools/src/registry.rs`, `maukode-core/src/mcp/tool.rs`). The dispatch path is unified.
What remains is **metadata and gating**, not a second registry:

- Add a tool **kind/source** (`native`, `mcp`, `memory`, `sandbox`) and a token-cost hint to
  `ToolDefinition`.
- Add **lazy/gated exposure**: only register a tool when its capability is relevant (e.g. memory
  tools when a scope is resolved; MCP tools only for connected servers), so prompt size stays
  bounded without per-source special-casing.
- Permission/audit stays centralized: one path for classify → approve → execute → stream.

Memory tools are a normal participant in that registry; they add no new parity work.

## 11. Out of scope for v1
- Embeddings / semantic search.
- User-global UI, team/tenancy, OAuth.
- Cloud sync or replication.
- Migration of existing agent-memory exports.
