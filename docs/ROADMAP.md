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
| Anthropic streaming | **done** | `protocols/anthropic.rs`; prompt caching (tools, stable system prompt, last message), `Usage` events, mid-stream `error` events fail the attempt (#87); thinking blocks are dropped |
| ChatGPT plan (Sign in with ChatGPT, Responses API) | **done** | `chatgpt_auth.rs`, `chatgpt_client.rs`, `maukode auth` |
| ChatGPT credentials on a remote daemon | **planned** | — |

### Tools, permissions, and isolation (`maukode-tools`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| File tools with path jail | **done** | `file.rs`, `grep.rs`, `security.rs` (`PathJail`) |
| Bash risk assessment and approval prompts | **partial** | `assess_bash_command` (inspects `bash -c`, `eval`, subshells, wrappers, `find -exec`); pattern-based, unknown commands are treated as safe (see backlog) |
| Word-by-word `bash_whitelist`, per-tool session approvals | **done** | `bash_whitelist_allows`, `PermissionManager::is_whitelisted` (#86) |
| Subagent tool calls go through the parent's permissions | **done** | `SubagentTool::with_permission_manager` (#86) |
| Repo-supplied MCP servers require a trusted project | **done** | `security.trusted_projects`, `maukode trust` (#86) |
| Private tool results (`[p]`) | **done** | `PermissionDecision::AllowPrivate`, `PrivateToolResult` |
| Bubblewrap sandbox for bash | **done** (Linux, opt-in) | `security.sandbox_mode = bubblewrap\|auto` |
| lawang execution cells (bubblewrap, gVisor, Firecracker) | **partial** (opt-in) | `exec.rs`, `execution.backend = lawang` (PR #84); daemon sessions only, not the in-process CLI/TUI paths; MCP servers stay on the host; one setting for all sessions |
| Network capability controls, secret broker | **planned** | — |
| Web search (DuckDuckGo, Brave, Tavily, SearXNG) and fetch | **done** | `web.rs`; fetch blocks link-local/metadata addresses and the daemon API (#86) |

### Daemon (`maukode-server`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| HTTP + resumable SSE API | **done** | `routes/`, `docs/API.md` |
| Bearer token auth, loopback Host guard, CORS | **done** | `middleware/auth.rs`, `routes/mod.rs`; CLI-started daemons always get a token, non-loopback binds require one (#86) |
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

Fixed in #87:
- prompt caching: tool definitions are sorted, the system prompt is split into a stable part and a
  per-turn suffix (date and recalled memories), and the Anthropic adapter sets `cache_control` on
  the last tool, the stable system prompt, and the last message;
- the server keeps each workspace's `ContextManager` (with its repo-map parse cache) and memory
  store across turns, and MCP tool lists are fetched once per connection;
- `save_session` writes only messages that changed (compared by content hash), on a blocking
  thread, and keeps `created_at`; `messages_audit` is keyed by content hash instead of the full
  text;
- read-only tool calls of one step run concurrently;
- `grep` runs on a blocking thread, honours `.gitignore`, skips binary and large files, and prints
  workspace-relative paths;
- `read_file` returns at most 2000 lines (and 2000 characters per line) unless `end_line` is given.

Remaining:

1. The tool registry itself is still rebuilt every turn (cheap now that the expensive parts are
   cached). MCP tool lists are not refreshed when a server reports `tools/list_changed`.
2. Recalled memories change the system prompt between turns, so on Anthropic the cached messages
   of the previous turn are not reused across turns (within a turn every step is a cache hit).
3. A failed automatic compaction is retried on every following step (one extra LLM call each).
4. Token estimation uses a chars/4 heuristic and ignores tool schemas; use the provider's reported
   `input_tokens` when available.
5. Streaming text is published one delta per event into a 500-slot broadcast channel; coalesce
   deltas to reduce SSE and redraw load. In the TUI, the streaming message is re-parsed on every
   frame.
6. Identical messages in one session are stored once in `messages_audit` (it deduplicates by
   content hash, as it did by text before).

### Reliability

Fixed in #87: LLM clients have a 30 s connect timeout and a 300 s read timeout (a stalled stream
is retried instead of hanging the turn); the Anthropic adapter reports mid-stream `error` events,
rejects malformed tool-argument JSON, and emits `LLMEvent::Usage`.

Remaining:

1. Around 400 `unwrap()`/`expect()` calls in non-test-looking code paths (rough count); audit
   against the `AGENTS.md` rule.

### Security

Fixed in #86: prefix-matched `bash_whitelist` (`cargo *` let `cargo build && rm -rf ~` through
in every mode), session approvals shared across tools, `bash -c`/`eval`/subshell/wrapper bypasses
of the risk assessment, subagents running without any permission manager, token-less local
daemons, non-loopback binds without a token, non-constant-time token comparison, `tauri://`
origins, repositories starting their own MCP servers, `fetch_web_content` reaching metadata
addresses and the daemon API, and a world-readable `config.json`.

Remaining risks, by design or not yet addressed:

1. **The risk assessment is a prompt filter, not a sandbox.** In `ask` mode a command it does not
   recognize runs without a prompt, including a script the agent wrote itself (`bash x.sh`,
   `cargo run`, `npm test`). Use `strict`/`review`, a bubblewrap sandbox, or a lawang cell when
   that matters.
2. **MCP tools never prompt in `ask` mode** (only `bash` is assessed); trusted projects and the
   global configuration decide which MCP servers exist.
3. **`fetch_web_content` can reach private LAN addresses** (only link-local/metadata addresses and
   the daemon API are blocked) and runs without a prompt.
4. The daemon and the CLI both rewrite `config.json`; a daemon saving its in-memory configuration
   can drop a `trusted_projects` entry that `maukode trust` added while it was running.

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
