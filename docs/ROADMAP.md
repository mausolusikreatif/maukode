# Roadmap and Feature Status

This is the **single source of truth for what is implemented**. The client documents
([WEB_UI_ROADMAP.md](./WEB_UI_ROADMAP.md), [TUI_ROADMAP.md](./TUI_ROADMAP.md)) describe product
direction and interaction design; they link here instead of keeping their own status.

Last audited against `master` at `eab1314` (October 20, 2026), after PR #83 (web UI rebuilt, Tauri
removed) and PR #84 (lawang execution backend).

**Labels:** **done**: shipped and reachable by users · **partial**: shipped with known gaps ·
**backend only**: daemon/API exists, no client surface · **planned**: no implementation yet.
Every **done** or **partial** claim names a file, route, or symbol as evidence. When you ship or
remove a feature, update its row in the same PR.

---

## Feature status

### Agent core (`maukode-core`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| Multi-step agent loop with retries for transient stream errors | **done** | `runner.rs` (`MAX_STREAM_ATTEMPTS`, resume of text-only turns) |
| Auto-continue after `max_tokens` cut-off | **done** | `runner.rs` (`MAX_AUTO_CONTINUATIONS`) |
| Cancellation, including mid-stream and during approval wait | **done** | `runner.rs` (`cancelled_results`) |
| Shadow git checkpoints, diff, undo | **done** | `checkpoint.rs`; `/diff`, `/undo`; web UI Review panel |
| Git worktree isolation per session | **done** | `worktree.rs`; web UI commit/merge in `ui/Review.tsx` |
| Context pruning and automatic/manual compaction | **done** | `compaction/`; `/compact` |
| Tree-Sitter repo map (Rust, TS/JS, Python, Go) | **done** | `repo_map/`; `get_codebase_map`; `/map` |
| `AGENTS.md` discovery and `maukode init` | **done** | `rules/` |
| MCP client (stdio) | **done** | `mcp/`; `/mcp` |
| Subagents | **done** | `subagent.rs`; `invoke_subagent` |
| Skills and plugins | **partial** | `skills.rs`, `plugin.rs`, `/api/v1/skills`, `/api/v1/plugins`; web UI Settings can enable/disable plugins and list skills; no skill authoring or slash-command use |
| Post-turn reflection and memory consolidation | **done** (opt-in) | `reflection.rs`, `consolidation.rs`; `agent.reflection`, `agent.consolidation` |
| Dev-server preview | **backend only** | `routes/preview.rs`, `preview` state in `state/store.ts`; no panel in the web UI |

### Providers (`maukode-llm`, `maukode-core`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| OpenAI-compatible streaming (OpenAI, OpenRouter, Ollama, DeepSeek, llm-kita, custom) | **done** | `protocols/openai.rs`, usage via `stream_options` |
| Anthropic streaming | **partial** | `protocols/anthropic.rs`; no prompt caching, no `Usage` event, thinking blocks dropped, mid-stream `error` events ignored (see backlog) |
| ChatGPT plan (Sign in with ChatGPT, Responses API) | **done** | `chatgpt_auth.rs`, `chatgpt_client.rs`, `maukode auth` |
| ChatGPT credentials on a remote daemon | **planned** | — |

### Tools, permissions, and isolation (`maukode-tools`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| File tools with path jail | **done** | `file.rs`, `grep.rs`, `security.rs` (`PathJail`) |
| Bash risk assessment and approval prompts | **partial** | `assess_bash_command`; pattern-based, unknown commands are treated as safe (see backlog) |
| Private tool results (`[p]`) | **done** | `PermissionDecision::AllowPrivate`, `PrivateToolResult` |
| Bubblewrap sandbox for bash | **done** (Linux, opt-in) | `security.sandbox_mode = bubblewrap\|auto` |
| lawang execution cells (bubblewrap, gVisor, Firecracker) | **partial** (opt-in) | `exec.rs`, `execution.backend = lawang` (PR #84); daemon sessions only, not the in-process CLI/TUI paths; MCP servers stay on the host; one setting for all sessions |
| Network capability controls, secret broker | **planned** | — |
| Web search (DuckDuckGo, Brave, Tavily, SearXNG) and fetch | **done** | `web.rs` |

### Daemon (`maukode-server`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| HTTP + resumable SSE API | **done** | `routes/`, `docs/API.md` |
| Bearer token auth, loopback Host guard, CORS | **done** | `middleware/auth.rs`, `routes/mod.rs` |
| Steering and prompt queueing | **done** | `/api/v1/sessions/{id}/steer`, `queued_prompts` |
| Live progress in the session list | **done** | `SessionListItem` (`turn_started_at`, `plan_done`, `last_activity`) |
| Session audit (permissions, mode changes, compactions) | **done** | `store/sqlite.rs` audit tables |
| Scheduled automations | **planned** | no schedule entities in the store or protocol |

### Web UI (`apps/desktop`)

The web UI was rebuilt from scratch in PR #83. Some capabilities of the previous UI were not carried
over; they are listed as **planned** again below.

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| Overview by urgency and cross-session approval inbox | **done** | `ui/Overview.tsx` |
| Session thread: messages, reasoning, tools, plans, subagents | **done** | `ui/Thread.tsx`, `state/thread.ts` |
| Permission dock with all four decisions | **done** | `ui/Dock.tsx` |
| Composer with `@` context chips (files, folders, symbols, changes), steering, queueing | **done** | `ui/Composer.tsx`, `components/composerContext.ts` |
| Change review, undo, worktree commit and merge | **done** | `ui/Review.tsx` |
| Session list: project grouping, search, pin, rename, delete | **done** | `ui/Sidebar.tsx`, `state/sessionFilters.ts`; pin state is browser-local |
| Session archive | **planned** | state exists in `state/sessionFilters.ts`, no UI |
| Settings: providers, ChatGPT login, permissions, sandbox, agent limits, memory, MCP/plugins/skills, theme | **done** | `ui/Settings.tsx` |
| Browser notifications for approvals and finished turns | **done** | `state/notify.ts` |
| Phone layout with bottom tabs and history-based Back | **done** | `App.tsx` |
| New session: folder text field with recent repositories, worktree toggle | **done** | `ui/Home.tsx`, `state/repositories.ts` |
| Command palette and shortcut sheet | **planned** | only `shortcutsOpen` state remains in `state/store.ts` |
| Open in editor / reveal in file manager | **planned** | removed with the Tauri shell |
| Discard worktree from the review panel | **planned** | branch can be discarded when deleting the session |
| Attachments (images, uploaded files, clipboard) | **planned** | — |
| Split/syntax-highlighted diff, line comments, per-hunk revert | **planned** | — |
| Dev-server preview panel | **planned** | backend exists |

### Terminal UI (`maukode-tui`)

| Milestone | Status | Evidence / gaps |
| :--- | :--- | :--- |
| M1 Plan widget | **done** | `PlanCreated`/`PlanStepUpdated`, `/plan`, `F2`, `draw_plan_widget` |
| M2 Steering and queueing | **planned** | input is rejected while `App::is_running`; daemon support exists |
| M3 Session explorer | **partial** | `/sessions` filtered selector; no pin/archive/filter chips |
| M4 Subagent stream cards | **done** | `Subagent*` events rendered in `ui.rs` |
| M5 Subagent trace modal | **done** | `ActiveModal::SubagentViewer` |
| M6 `@file` / `#symbol` autocomplete | **planned** | — |
| Concurrent subagent switching, split-pane layout | **planned** | — |

---

## Backlog from the October 2026 code audit

Ordered by impact within each group. Each item names where the problem is.

### Cost and latency

1. **Prompt caching never hits.**
   - The Anthropic adapter sends no `cache_control` (`protocols/anthropic.rs`).
   - `ToolRegistry::definitions()` iterates a `HashMap`, and the registry is rebuilt every turn
     (`routes/sessions.rs`), so tool order changes between requests. Sort the definitions.
   - The system prompt starts with the current time to the second (`context.rs`), and the repo map
     and knowledge recall are also part of it. Keep the stable prefix first and move the parts that
     change to the end.
2. **Work repeated on every turn.** The server rebuilds the tool registry, registers MCP tools,
   creates a new `ContextManager` (its `RepoMapGenerator` cache starts empty, so the whole repo is
   parsed again), and opens the memory store, for every prompt. Cache these per session or
   directory, as is already done for checkpoint managers.
3. **`save_session` rewrites the whole history** (`store/sqlite.rs`) every ~2 s during a turn,
   synchronously on the async runtime while holding the session lock, and resets every message's
   `created_at`. Make it append-only with an ordinal and run it on a blocking thread.
   `messages_audit` uses `UNIQUE(session_id, parts_json)`: this indexes the full text and drops
   identical messages. Use an ordinal or a content hash instead.
4. **Tools run one at a time** (`runner.rs`). Run read-only calls from the same step concurrently.
5. **`grep` blocks the runtime** (`grep.rs`): synchronous `walkdir` + `read_to_string`, ignores
   `.gitignore` although the `ignore` crate is a dependency, no file-size limit, absolute paths in
   output.
6. **`read_file` has no default limit**: large files are read in full and only cut by the pruner.
7. A failed automatic compaction is retried on every following step (one extra LLM call each).
8. Token estimation uses a chars/4 heuristic and ignores tool schemas; use the provider's reported
   `input_tokens` when available.
9. Streaming text is published one delta per event into a 500-slot broadcast channel; coalesce
   deltas to reduce SSE and redraw load. In the TUI, the streaming message is re-parsed on every
   frame.

### Reliability

1. **No HTTP timeouts** on any LLM client (`reqwest::Client::new()` in `anthropic.rs`,
   `openai.rs`, `chatgpt_client.rs`). A half-open stream hangs the turn indefinitely.
2. Anthropic: mid-stream `error` events (e.g. `overloaded_error`) end the stream as if it
   succeeded; malformed tool-argument JSON silently becomes `{}`; no `LLMEvent::Usage` is emitted.
3. Around 400 `unwrap()`/`expect()` calls in non-test-looking code paths (rough count); audit
   against the `AGENTS.md` rule.

### Security

1. **`bash_whitelist` matches by string prefix** (`PermissionManager::is_whitelisted`): a pattern
   `cargo *` becomes the prefix `cargo`, so `cargo build && rm -rf ~` (or `cargox …`) is
   whitelisted and skips approval in every mode, including `strict`. `cargo *` is in the default
   whitelist. Match per subcommand on token boundaries, and never whitelist a compound command.
2. **Bash risk assessment defaults to safe.** `bash -c "…"`, `sh -c`, `python -c`, `perl -e`,
   `eval`, and command substitution are not inspected, so destructive commands wrapped this way run
   without a prompt in `ask` mode. Treat interpreters with inline code as at least a warning, or
   switch to an allowlist.
3. **A non-loopback bind without a token is accepted** (`lib.rs`): the Host guard is turned off and
   nothing requires authentication. Refuse to start in that case.
4. The local daemon without a token is reachable by every local user; consider a generated token
   in a 0600 file or a Unix socket.
5. The token comparison is not constant-time (`middleware/auth.rs`).
6. CORS and the Host guard still accept `tauri://` origins (`routes/mod.rs`), which no client uses
   any more.

### Features

1. Accurate usage and cost per session (needs the Anthropic `Usage` fix and caching).
2. Extended thinking for Anthropic, including thinking-signature round trips.
3. Image input (`ContentPart` is text-only).
4. Multi-file edits (`apply_patch`).
5. Hooks (e.g. format after edit) and pattern-based permission rules instead of exact-match
   session whitelisting.
6. A JSON output mode for `maukode run` for CI and scripts.
7. TUI parity: steering/queueing and `@file` autocomplete (M2, M6).
8. Restore web UI features lost in the rebuild (command palette, open in editor, archive).

---

## Notes

- **llm-kita credential (October 2026).** A live llm-kita key was once embedded in the default
  configuration and is in Git history. It was removed; defaults read `LLMKITA_API_KEY`. The key must
  be revoked in the llm-kita dashboard, and old user `config.json` files may still contain it.
- **llm-kita stream termination.** The gateway can close the stream right after a chunk with
  `finish_reason`, without `[DONE]`. The OpenAI-compatible adapter treats an EOF after a valid
  finish chunk as success; a regression test covers it.
- **TUI scroll limit.** Ratatui's `Paragraph` scroll offset is a `u16`, so histories beyond 65,535
  wrapped rows need a windowing implementation to reach the newest rows.
