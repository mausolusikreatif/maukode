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
| MCP client (stdio) | **done** | `mcp/`; `/mcp`; tool lists refreshed after `notifications/tools/list_changed`, server `ping` answered |
| Subagents | **done** | `subagent.rs`; `invoke_subagent` |
| Skills and plugins | **partial** | `skills.rs`, `plugin.rs`, `/api/v1/skills`, `/api/v1/plugins`; web UI Settings can enable/disable plugins and list skills; no skill authoring or slash-command use |
| Post-turn reflection and memory consolidation | **done** (opt-in) | `reflection.rs`, `consolidation.rs`; `agent.reflection`, `agent.consolidation` |
| Dev-server preview | **backend only** | `routes/preview.rs`, `preview` state in `state/store.ts`; no panel in the web UI |

### Providers (`maukode-llm`, `maukode-core`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| OpenAI-compatible streaming (OpenAI, OpenRouter, Ollama, DeepSeek, llm-kita, custom) | **done** | `protocols/openai.rs`, usage via `stream_options` |
| Anthropic streaming | **done** | `protocols/anthropic.rs`; prompt caching (tools, system prompt, last message) over an append-only history, `Usage` events with cached tokens included in `input_tokens`, mid-stream `error` events fail the attempt |
| Extended thinking (Anthropic) | **done** | `thinking_param` in `protocols/anthropic.rs`, `models.<id>.thinking`; adaptive on Claude 4.6+, budget for older models, effort, summarized display; thinking blocks replayed verbatim with `drop_block` for stale ones |
| Image input | **done** | `ContentPart::Image`; Anthropic, OpenAI-compatible and ChatGPT adapters; `POST /sessions/{id}/prompt` `images`; web UI attach/paste/drop in `ui/Composer.tsx`; not from `read_file`, the TUI, or `maukode run` |
| Session usage and cost | **done** | `pricing.rs` (built-in Claude prices, `models.<id>.pricing`), `session_usage` table, `usage_requests` table (one row per main-agent request; `SessionUsage.context_tokens` is the latest request's input size), `LLMEvent::SessionUsage`, snapshot `usage`; web UI composer ("konteks" and "total"), TUI status bar (`ctx`); subagent requests are in the totals only |
| ChatGPT plan (Sign in with ChatGPT, Responses API) | **done** | `chatgpt_auth.rs`, `chatgpt_client.rs`, `maukode auth` |
| ChatGPT credentials on a remote daemon | **planned** | — |

### Tools, permissions, and isolation (`maukode-tools`)

| Feature | Status | Evidence / gaps |
| :--- | :--- | :--- |
| File tools with path jail | **done** | `file.rs`, `grep.rs`, `security.rs` (`PathJail`) |
| Multi-file edits (`apply_patch`) | **done** | `patch.rs`; add, update, delete and move files in one all-or-nothing call; synced to lawang VMs like the other file tools |
| Pattern-based permission rules | **done** | `security.permission_rules`, `PermissionManager::rule_action` |
| Hooks (`pre_tool_use`, `post_tool_use`, `turn_end`) | **done** | `hooks.rs`; global config, and `.maukode/hooks.json` for trusted projects |
| Bash risk assessment and approval prompts | **partial** | `assess_bash_command` (inspects `bash -c`, `eval`, subshells, wrappers, `find -exec`); pattern-based, unknown commands are treated as safe (see backlog) |
| Word-by-word `bash_whitelist`, per-tool session approvals | **done** | `bash_whitelist_allows`, `PermissionManager::is_whitelisted` (#86) |
| MCP tools confirmed in `ask` mode | **done** | `Tool::requires_confirmation`, `McpTool::with_read_only` (`readOnlyHint` skips the prompt), `PermissionManager::whitelist_session_tool` |
| Subagent tool calls go through the parent's permissions | **done** | `SubagentTool::with_permission_manager` (#86) |
| Repo-supplied MCP servers require a trusted project | **done** | `security.trusted_projects`, `maukode trust` (#86) |
| Private tool results (`[p]`) | **done** | `PermissionDecision::AllowPrivate`, `PrivateToolResult` |
| Bash output filters | **partial** | `maukode-tools/src/compress.rs`: built-in filters for successful `cargo build/check/test/clippy/doc` and `git status/push/pull/fetch` (single plain commands only, via `simple_command_words`); opt out with `MAUKODE_NO_COMPRESS=1`; planned: user/project filter files, `ls`/`find`/`grep`, base64 blob placeholder |
| Self-update (`maukode update`) | **done** (Linux x86_64, `install.sh` installs) | `maukode-core/src/update.rs`, `run_update` in `maukode-cli`; GitHub Release tarball with sha256 check, web UI swapped before the binary, daemon restarted; `--check`, `--version`; no periodic "update available" notice yet |
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
| Session archive | **done** | `PUT /sessions/{id}/archive`, `sessions.archived`; sidebar Archive toggle and Arsipkan/Pulihkan; browser-local archives move to the daemon once |
| Settings: providers, ChatGPT login, permissions, sandbox, agent limits, memory, MCP/plugins/skills, theme | **done** | `ui/Settings.tsx` |
| Browser notifications for approvals and finished turns | **done** | `state/notify.ts` |
| Phone layout with bottom tabs and history-based Back | **done** | `App.tsx` |
| New session: folder text field with recent repositories, worktree toggle | **done** | `ui/Home.tsx`, `state/repositories.ts` |
| Command palette and shortcut sheet | **done** | `ui/Palette.tsx` (`Ctrl/Cmd+K`: navigation, session actions, every session by title), shortcut sheet on `?` |
| Open in editor | **done** | `POST /sessions/{id}/open`, `editor.command` (default `code --goto {path}:{line}`); buttons in the review panel and on file tool calls; runs on the daemon's machine. No "reveal in file manager" |
| Discard worktree from the review panel | **planned** | branch can be discarded when deleting the session |
| Attachments | **partial** | images (pick, paste, drop) in `ui/Composer.tsx`; no other file types |
| Split/syntax-highlighted diff, line comments, per-hunk revert | **planned** | — |
| Dev-server preview panel | **planned** | backend exists |

### Terminal UI (`maukode-tui`)

| Milestone | Status | Evidence / gaps |
| :--- | :--- | :--- |
| M1 Plan widget | **done** | `PlanCreated`/`PlanStepUpdated`, `/plan`, `F2`, `draw_plan_widget` |
| M2 Steering and queueing | **done** | `App::steer`, `App::queue_prompt` (`Alt+Enter` / `/steer`, `Enter` queues while a turn runs, `/queue`); the queue is kept in the TUI and continues after a completed turn |
| M3 Session explorer | **partial** | `/sessions` filtered selector; no pin/archive/filter chips |
| M4 Subagent stream cards | **done** | `Subagent*` events rendered in `ui.rs` |
| M5 Subagent trace modal | **done** | `ActiveModal::SubagentViewer` |
| M6 `@file` / `#symbol` autocomplete | **partial** | `@file` completion in `mention.rs` (fuzzy, `.gitignore`-aware, `maukode_tools::workspace_files`); no `#symbol` |
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

Fixed in #88:
- the per-turn context (date, recalled memories) is sent after the conversation, appended to the
  last user message as a `<maukode-context>` note, so on Anthropic, OpenAI-compatible providers
  and the ChatGPT plan the whole earlier history stays a cached prefix across turns;
- the context-window check uses the size the provider reported for the previous step plus an
  estimate for newer messages only, and the full estimate includes the tool schemas; Anthropic's
  `input_tokens` now includes cached tokens, as with the other providers;
- after an automatic compaction (or a failed attempt) the next one waits until the conversation
  has grown by 10% of the context window, instead of running again on every step;
- the daemon merges consecutive text, reasoning and tool-argument deltas for up to 50 ms before
  publishing them (fewer SSE frames, and the 2000-event replay log covers much longer replies);
  the TUI redraws streamed text on its 50 ms tick instead of once per delta;
- MCP tool lists are fetched again after a server sends `notifications/tools/list_changed`.

Remaining:

1. The tool registry is rebuilt every turn. The expensive parts (MCP connections and tool lists,
   the workspace context) are cached, so this costs little.
2. Identical messages in one session are stored once in `messages_audit` (it deduplicates by
   content hash, as it did by text before).

### Reliability

Fixed in #87: LLM clients have a 30 s connect timeout and a 300 s read timeout (a stalled stream
is retried instead of hanging the turn); the Anthropic adapter reports mid-stream `error` events,
rejects malformed tool-argument JSON, and emits `LLMEvent::Usage`.

Fixed in #88: the earlier estimate of about 400 `unwrap()`/`expect()` calls counted test code.
Outside tests there were 13: the HTML-to-Markdown regexes in `web.rs`, compiled again on every
call (and `strip_html_tags` once per link and list item), and a constant address parse. The
regexes are now compiled once (`static_regex!`); the remaining `expect` is on constant patterns
covered by the tests. MCP servers that send a request (such as `ping`) now get an answer instead
of having it mistaken for a response.

### Security

Fixed in #86: prefix-matched `bash_whitelist` (`cargo *` let `cargo build && rm -rf ~` through
in every mode), session approvals shared across tools, `bash -c`/`eval`/subshell/wrapper bypasses
of the risk assessment, subagents running without any permission manager, token-less local
daemons, non-loopback binds without a token, non-constant-time token comparison, `tauri://`
origins, repositories starting their own MCP servers, `fetch_web_content` reaching metadata
addresses and the daemon API, and a world-readable `config.json`.

Fixed in #88: MCP tools are confirmed in `ask` mode (unless marked `readOnlyHint`), with "allow
for this session" covering the tool; only `maukode trust` writes `security.trusted_projects`, so
a daemon saving its configuration no longer drops a trust added while it ran.

Remaining risks, by design or not yet addressed:

1. **The risk assessment is a prompt filter, not a sandbox.** In `ask` mode a command it does not
   recognize runs without a prompt, including a script the agent wrote itself (`bash x.sh`,
   `cargo run`, `npm test`). Use `strict`/`review`, a bubblewrap sandbox, or a lawang cell when
   that matters.
2. **`fetch_web_content` can reach private LAN addresses** (only link-local/metadata addresses and
   the daemon API are blocked) and runs without a prompt. Blocking it alone would not help while
   `curl` in `bash` reaches the same addresses without a prompt; the fix is the planned network
   capability controls.
3. **`readOnlyHint` comes from the MCP server.** A server that marks a mutating tool read-only
   skips the `ask` prompt; which servers run is still decided by the global configuration and
   trusted projects.

### Features

Done in #89: usage and cost per session, extended thinking for Anthropic (with signature round
trips), and image input. Done in #90: `apply_patch`, hooks, and pattern-based permission rules.
Done in #91: `maukode run --output json|stream-json` and TUI steering, queueing and `@file`
completion. Done in #92: the web UI command palette and shortcut sheet, session archive on the
daemon, and open in editor. Remaining:

1. Turn an approval into a permission rule from the prompt ("always allow `npm run *`").
2. Images from the TUI, `maukode run`, and `read_file`; `#symbol` completion in the TUI.

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
