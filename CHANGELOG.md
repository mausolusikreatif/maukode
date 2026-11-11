# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.3] - 2026-11-11

### Added

- Windows x86_64 release: each release now carries `maukode-windows-x86_64-<tag>.zip` with a `.sha256` file, built by `windows-build.yml`. `install.ps1` downloads it, verifies the checksum, and installs for the current user into `%LOCALAPPDATA%\Programs\MauKode` (no administrator rights; `-Uninstall` removes it). CI now also compiles the workspace on Windows.

### Changed

- On Windows, the `bash` tool and hooks use Git for Windows' `bash.exe` (or the path in `MAUKODE_BASH`) instead of whichever `bash` comes first on `PATH`, which is often the WSL launcher. `npm`, `npx`, and other `.cmd` launchers now resolve through `PATHEXT`, so the preview and MCP servers started with `npx` can start.
- On Windows, the background daemon starts detached without a console window, and concurrent starters wait on a lock as they do on Unix.

## [0.7.2] - 2026-11-11

### Changed

- The TUI palette now matches the web UI's dark-mode tokens (`--ink`, `--ink-2`, `--line`, `--surface`, `--cobalt`, `--hold`, `--green`, `--red`). Each accent color has one meaning: amber means the agent is waiting for you, red means deny or failure, and green means success or an added line. The mint, cyan, blue, and purple colors are gone; Markdown headings and tool names are told apart by weight instead of color. Modal borders are now flat (`--line`), and diffs use the `--add-bg`/`--del-bg` backgrounds.
- The web UI folder moved from `apps/desktop` to `apps/web` (the npm package is now `maukode-web`). The manual CI input `run_desktop` is now `run_web`, and the `desktop` job is now `web`.
- Everything is in English now: the changelog, the docs, and the web UI strings (previously Indonesian). Settings sections have English ids (`connection`, `permissions`, `execution`, `extensions`, `appearance`).

## [0.7.1] - 2026-10-25

### Fixed

- The codebase map no longer walks an entire home directory: the walk stops at 5,000 files or 5 seconds, and the system prompt carries no map when the working directory is the home directory or `/`. Previously a session started from home burned CPU for a long time.

### Added

- Chat-only mode: in the web UI's "New session" form, emptying "Project folder" (or pressing "Chat only") starts a conversation without a project folder. The agent only has web search and memory: no file or shell access and no codebase map. Chat sessions are grouped under "Chat", and their empty folder is deleted together with the session. API: `POST /sessions` with `chat: true`.

## [0.7.0] - 2026-10-25

### Added

- `bash` output filters now cover `git log` (truncated in the middle), `pytest`, `go test`, and `pip install`.
- Extra filters can be written in `.maukode/filters.json` (project) and `filters.json` next to `config.json` (user): program, subcommand, line patterns to drop, and a line limit.
- An `install.sh` installation now tells you when a newer release exists (once a day, in the background): after the TUI closes or at `maukode server`. Turn it off with `MAUKODE_NO_UPDATE_CHECK=1`.
- Web UI: the "Prompt" button in the session header shows the fixed cost per request (system prompt + tool schemas), the tools with the largest schemas, and the text of the system prompt last sent to the model.

## [0.6.0] - 2026-10-25

### Fixed

- **Cached tokens are read from OpenAI-compatible providers**: usage now reads
  `prompt_tokens_details.cached_tokens` (OpenAI, GLM), `prompt_cache_hit_tokens` (DeepSeek), and
  `cached_tokens` (Moonshot). Before, the cache was always recorded as 0. When a provider does not
  report it, the value stays "unknown" instead of 0.
- **The web UI works after `install.sh`**: the daemon now also looks for the web UI in
  `<prefix>/share/maukode/web` and `~/.local/share/maukode/web`, so `maukode server` serves it
  right away without `--web-dir`. The startup message shows the folder in use, or says when none
  was found.
- **The binary version follows the release**: the workspace version was raised to 0.5.0, and the
  release workflow fails when a `v*` tag does not match the version in `Cargo.toml`.

### Added

- **Old tool results are pruned without waiting for the context window to fill**: when a provider
  has no prompt cache (it never reported a cache hit), tool results older than the latest window
  are truncated head/tail as soon as the pile reaches about 6k tokens, and the "old" cutoff only
  moves every 6 assistant messages so the parts that were already pruned do not change in between.
  With a cache, the earlier behavior still applies (prune once the context is 60% full), because
  rewriting old messages throws away the cache. Before, pruning was practically never active on
  models with a 1 million token window.
- **System prompt and tool schema snapshots**: the prompt and tool schemas sent to the model are
  now stored once per unique combination (the `prompt_snapshots` table, deduplicated by hash) and
  referenced by every request through `usage_requests.snapshot_hash`. `GET
  /api/v1/sessions/{id}/system-prompt` shows the prompt, a token estimate, and the size of each
  tool from the session's latest request; snapshots that are no longer referenced are deleted
  together with their session.
- **Context size vs. total in session usage**: the web UI shows "context 30k" (the input of the
  latest request) next to "total 546k" (the sum of all requests, because the history is sent again
  with every request), and the TUI status bar shows `ctx`. Every main-agent request is now stored
  in the `usage_requests` table (model, input/output tokens, cache), so token growth can be
  audited. Subagent requests only count toward the total.
- **`bash` output is compressed for `cargo` and `git`**: for single commands that exit with code 0,
  progress lines (`Compiling`, `Downloading`, `test ... ok` lines, `git status` hints, `git push`
  progress) are dropped before they enter the context, and `cargo` results longer than 400 lines
  are truncated in the middle. Failed output is never changed, chains/pipes/redirects are not
  filtered, and `MAUKODE_NO_COMPRESS=1` turns it off.
- **`maukode update`**: updates an `install.sh` installation to the latest GitHub release
  (`--check` only reports, `--version vX.Y.Z` picks a release). The tarball's sha256 is verified,
  the web UI is replaced first and then the binary is replaced with `rename`, and a running daemon
  is restarted. A binary from `cargo install` is refused with a hint, and so is any platform other
  than Linux x86_64.
- **One-line installer**: `curl -fsSL https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.sh | bash`
  downloads the latest release (or `--version vX.Y.Z`), verifies the sha256, and installs it to
  `~/.local`. `--uninstall` removes it.

### Performance

- **Prompt caching is actually used**: the order of tool definitions is now fixed (sorted by name),
  the current time is no longer at the start of the system prompt (only the date, at the end
  together with the recalled memories), and the Anthropic adapter sets `cache_control` on the last
  tool, the stable system prompt, and the last message. Within one turn, every step reads the
  previous request from the cache.
- **Less work per prompt**: the daemon keeps the `ContextManager` (with the repo map parse cache)
  and the memory store per workspace between turns, so the repo is not parsed again for every
  prompt. The MCP tool list is fetched once per connection.
- **`save_session` writes only the messages that changed** (compared by content hash), on a
  blocking thread, and the `created_at` of old messages is no longer reset. `messages_audit` is now
  keyed by content hash instead of the full text; old databases are migrated automatically when
  opened.
- **Read-only tools run in parallel**: when all tool calls in one step only read (`read_file`,
  `grep`, `glob`, `web_search`, …), they all run at the same time.
- **`grep` no longer blocks the runtime**, honors `.gitignore`, skips binary files and files over
  1 MB, and prints paths relative to the workspace.
- **`read_file` without `end_line` returns at most 2000 lines** (and 2000 characters per line),
  with a hint on how to read the rest.
- **The prompt cache survives between turns**: the per-turn context (the date and the recalled
  memories) is now sent at the end of the last user message as a `<maukode-context>` note instead
  of in the system prompt. The earlier conversation history stays a cached prefix on Anthropic,
  OpenAI-compatible providers, and the ChatGPT plan.
- **More accurate context size estimates**: the context window check uses the size the provider
  reported for the previous step, plus an estimate for newer messages only; the full estimate now
  also counts the tool schemas. Anthropic's `input_tokens` now includes cached tokens, like the
  other providers.
- **Automatic compaction does not repeat at every step**: after a compaction (successful or
  failed), the next one waits until the conversation has grown by 10% of the context window.
- **Streaming deltas are merged**: the daemon merges consecutive text, reasoning, and tool argument
  deltas (at most 50 ms) before publishing them, so there are far fewer SSE frames and the replay
  log covers much longer replies. The TUI draws streamed text on a 50 ms tick instead of once per
  delta.
- **The HTML-to-Markdown regexes are compiled once** in `fetch_web_content` instead of on every
  call (and per link or list item).

### Fixed

- **The conversation history is now truly append-only**:
  - The per-turn context note (date and memories) is stored in its prompt message and is no longer
    moved to the last message at every step.
  - The system prompt is frozen per session (rebuilt only after a compaction).
  - Pruning of old tool output only starts when the context is 60% full.
  - As a result the prompt cache stays in use, and Claude's thinking blocks stay valid. Newer
    models reject thinking blocks whose history was changed.
- **Compaction drops thinking blocks from the messages it keeps**, because those blocks are tied to
  the history that was replaced by the summary.

- **A stalled LLM stream no longer hangs the turn**: all LLM clients have a 30 second connect
  timeout and a 300 second read timeout; a stream that stays silent for too long is retried.
- **Anthropic adapter**: an `error` event in the middle of a stream (for example
  `overloaded_error`) is now an error (and retried when transient) instead of being treated as
  finished; malformed tool argument JSON becomes an error instead of `{}`; token usage (including
  cache read/write) is reported through the `Usage` event.
- **MCP**: the tool list is fetched again after a server sends `notifications/tools/list_changed`,
  and requests from the server (for example `ping`) are now answered instead of being mistaken for
  a response.

### Security

- **The bash whitelist can no longer be piggybacked by other commands**: `bash_whitelist` entries
  used to be matched as a string prefix, so `cargo *` (in the default whitelist) also let
  `cargo build && rm -rf ~` through without approval, even in `strict` mode. An entry now only
  matches **one simple command** (no `;`, `&&`, `|`, redirection, `$(…)`, backticks, subshell, or
  newline), matched word by word, only for the `bash` tool, and it never lets through a command
  that is assessed as dangerous.
- **"For this session" approval is now per tool**: approving `memory_get {"id":1}` no longer also
  lets `memory_delete {"id":1}` through.
- **The bash risk assessment looks inside wrapped commands**: `bash -c '…'`, `sh -c`, `eval`,
  subshells `( … )`, `<( … )`, `{ …; }`, `if … then`, brace expansion `{rm,-rf,x}`, here-strings
  `bash <<< '…'`, wrappers (`timeout`, `nice`, `nohup`, `xargs`, `busybox`, …), `find -exec`, and
  `git -C dir …` are now inspected. Code that cannot be inspected (`python -c`, `node -e`,
  `perl -e`, a shell/interpreter reading from stdin, a command name from a variable, `awk` with
  `system()`) is at least a warning, so `ask` mode asks for approval. The same goes for writes
  outside the workspace (`>> ~/.bashrc`, `tee /etc/…`, `cp`/`mv` to an absolute path),
  `git checkout -- .`, `git restore`, and `git push --force-with-lease`/`--delete`. Reading
  `daemon.json` or `chatgpt_auth.json` and calling the local daemon API
  (`curl localhost:4096/api/v1/…`) is considered dangerous.
- **Subagents now ask for approval**: subagent tool calls used to run without any permission
  manager. A subagent now uses the permission mode and session whitelist of its parent session,
  and its permission requests appear in the parent session.
- **The local daemon always uses a token**: `maukode` and `maukode server start` generate a random
  token when none is given, store it in `daemon.json` (readable by the owner only), and local
  clients use it automatically. `maukode server start|status|restart` prints the web UI address
  with `#token=…`. Opening `http://127.0.0.1:4096` without a token now asks for the token.
- **`maukode server` refuses a non-loopback address without a token** (for example
  `--bind 0.0.0.0:4096`); an empty token counts as no token. Tokens are compared in constant time.
  The `tauri://` origin is no longer accepted.
- **MCP servers from a repo do not start on their own**: a project's `mcp.json` /
  `.maukode/mcp.json` is only used when the folder is in `security.trusted_projects`. Add it with
  `maukode trust` (revoke with `maukode trust --revoke`). MCP servers from the global
  configuration run as usual.
- **`fetch_web_content` refuses cloud metadata addresses and the daemon API**: link-local
  addresses (`169.254.169.254`, `fe80::/10`), other metadata addresses, and `localhost/api/…` are
  refused, including through redirects and DNS names. The body is read incrementally and stops at
  a 2 MB limit.
- **`config.json` is now readable by its owner only** (0600) because it contains API keys.
- **MCP tools now ask for approval in `ask` mode**: MCP tools run code that MauKode cannot
  inspect, so every call now asks for permission, except tools the server marks with
  `readOnlyHint`. "Allow for this session" applies to that tool with any arguments.
- **`maukode trust` can no longer be silently undone by the daemon**: only `maukode trust` writes
  `security.trusted_projects`. Other saves (the daemon's settings API, the CLI) keep the list that
  is on disk, so a daemon that saves its old configuration from memory no longer removes a project
  that was trusted while the daemon was running.

### Added

- **Web UI: command palette, session archive, and open in editor**:
  - `Ctrl/Cmd+K` opens a palette to switch sessions (search by title), navigate, and run session
    actions (view changes, stop the agent, archive). `?` shows the list of shortcuts.
  - The session archive is now stored in the daemon (`PUT /sessions/{id}/archive`), so it is the
    same on every device; old browser archives are moved over automatically. The sidebar has an
    Archive toggle and Archive/Restore buttons.
  - "Open in editor" in the changes panel and on file tool calls opens the file with
    `editor.command` (default `code --goto {path}:{line}`) on the machine where the daemon runs.
- **`maukode run --output json|stream-json`** for CI and scripts. `json` mode prints one result
  object (final text, stop reason, tool calls, denied permissions, errors, and usage with cost).
  `stream-json` mode prints every event as one JSON line, then the result object. In both modes,
  calls that need permission are denied automatically; `--permission-mode` changes that for the
  run. Exit code 1 when the turn fails.
- **TUI: steering, queueing, and `@file` autocomplete**:
  - `Enter` while the agent is working queues the prompt, which is then sent in order after the
    turn finishes. `/queue` shows, runs, or clears the queue.
  - `Alt+Enter` or `/steer` inserts guidance into the running turn.
  - Typing `@` opens a list of workspace files with fuzzy matching that honors `.gitignore`.
- **`apply_patch` tool**: adds, updates, deletes, and moves several files in one call with the
  `*** Begin Patch` format. All hunks are checked first, so a patch is applied entirely or not at
  all.
- **Pattern-based permission rules** (`security.permission_rules`): allow, ask, or deny tool calls
  by tool name, bash command pattern, or path glob, in every permission mode. `deny` always wins.
- **Hooks** (`hooks`): shell commands for `pre_tool_use` (can block), `post_tool_use` (for example
  a formatter after an edit; failure output is passed on to the model), and `turn_end`. Hooks from
  a project's `.maukode/hooks.json` only run for trusted projects.
- **Token usage and cost per session**: the token totals (input, output, cache read/write) and the
  cost of every session are stored, including subagent usage. The totals show in the web UI
  composer and the TUI status bar. Built-in prices exist for current Claude models; other models
  can be priced through `models.<id>.pricing` in the config. The daemon sends `session_usage`
  events, and the session snapshot now has a `usage` field.
- **Extended thinking for Anthropic**: adaptive thinking is on automatically for Claude 4.6 and up,
  with a fixed budget for older models. Effort and the summary display can be set per model
  through `models.<id>.thinking`. Thinking text shows as the reasoning, and the thinking blocks
  with their signatures are sent back intact in the next step.
- **Image input**: PNG, JPEG, GIF, or WebP images (at most 5 MB, 20 per message) can be attached
  in the web UI with the button, paste, or drag, and are sent to Anthropic, OpenAI-compatible
  providers, and the ChatGPT plan. API: `images` in `POST /sessions/{id}/prompt`.

- **Where commands run can be chosen (`ExecBackend`)**: the `bash` tool no longer always runs a
  local process. The new `execution` setting selects **Host** (as before) or **Lawang**: an
  isolated cell owned by the lawang daemon (a separate program, used over HTTP) with the
  `process` (bubblewrap), `sandbox` (gVisor), or `vm` (Firecracker) level. An old configuration
  without an `execution` section keeps running on the host.
  - One cell per session, created at the first command and deleted together with its session. The
    project folder is mounted into the cell at `/work`; for a VM, the folder is copied in and
    brought back. The file tools (`read_file`, `grep`, `glob`, `write_file`, `edit_file`) still
    work in the host folder and are wrapped in `SyncedTool` so changes on both sides become
    visible to each other. Files that exist only on the host are never deleted by the sync.
  - Subagents run commands in the same place as their parent session.
  - A daemon that cannot be reached, a wrong token, or a broken configuration (for example a VM
    without an image) makes commands fail with an explanation. **Commands never silently fall back
    to the host.**
  - API: `GET|PUT /api/v1/config/execution` and `GET /api/v1/config/execution/status` (asks the
    daemon which isolation levels are available). The token is never sent back.
  - UI: an **Execution** section in Settings and a label (Host / Isolated process / Sandbox / VM)
    in the session header.
  - Limits: MCP servers still run on the host; the CLI/TUI do not use this backend yet; the setting
    applies to all sessions at once.

### Changed

- **The documentation was tidied up to match the code**: the README was rewritten (the
  `maukode-memory` crate, the web UI, memory, subagents, the lawang execution backend, the full
  list of tools and slash commands, and an honest explanation of security: the path jail only
  covers the file tools and is not a sandbox for `bash`).
  - Feature status now lives only in `docs/ROADMAP.md`, including the backlog from the code audit
    (performance, reliability, security, features). `docs/ROADMAP_AUDIT.md` was deleted.
  - `docs/DESKTOP_ROADMAP.md` became `docs/WEB_UI_ROADMAP.md`, with no Tauri leftovers.
  - `docs/CONFIGURATION.md` follows the real schema (`providers` is a list with `kind`, the `agent`
    and `execution` blocks; the `compaction` block that never existed was removed).
  - `docs/ARCHITECTURE.md`, `docs/TUI_ROADMAP.md`, `docs/CI_TEST_PLAN.md`, and
    `apps/desktop/README.md` were updated; the release skill in `.maukode/skills/learned/` no
    longer tells you to change Tauri files.
  - `AGENTS.md` records the documentation language rule and that feature status is kept in
    `docs/ROADMAP.md`.

## [0.4.0] - 2026-10-15

Minor release. The agent step default goes from 50 to 2000, the CLI `--max-steps` delivery is fixed, and llm-kita streams are retried.

### Fixed

- **CLI `--max-steps` is actually sent to the daemon**: `ServerClient::set_agent_config`
  POSTed to the wrong path (`/api/v1/agent/config`, always a 404) while the CLI caller
  swallowed the error with `let _ =`, so the override always failed silently.
  It now targets `/api/v1/config/agent` and a failure is passed on as an
  explicit error (`Failed to apply --max-steps {n}: ...`).
- **llm-kita LLM streams cut off in the body are retried**: `LLMError::is_transient()` now
  recognizes reqwest/hyper messages such as `error decoding response body: stream
  closed before response completed`, so a turn that is cut off midway
  is resumed (at most 4 attempts) instead of failing right away.

### Changed

- **Default `max_steps` 50 → 2000** (`AgentConfig` and `RunnerConfig::default`).
  The server validation limit of `POST /config/agent` rose from `1..=200` to `1..=10000`.
  An existing saved config (`config.json`) is still honored and is not
  force-migrated; reset it once with `--max-steps`, Settings, or by editing the file.

## [0.3.2] - 2026-10-14

Patch release. Removes the OpenCode config sync that overrode the user's model choice.

### Added

- **Memory Consolidation Tier 4 (Abstraction Synthesis)**: after merging duplicates,
  the consolidation engine can now synthesize high-level abstraction memories
  from patterns across memories in the same taxonomy branch. New abstractions are written
  with `[[Title]]` links back to the source memories. Controlled by
  `ConsolidationConfig.abstraction` and `abstraction_min_memories`.
- **Consolidation Settings UI**: added consolidation and abstraction toggles to the
  Settings page of the desktop app.

### Removed

- **OpenCode config sync**: removed `sync_from_opencode_if_present()` entirely.
  This function used to read `~/.config/opencode/opencode.json` at every startup
  and override `active_model` and the provider `default_model` with the values from
  OpenCode (for example `"auto"`), then save the overwritten config
  back to disk. MauKode is a standalone project and should not read OpenCode's
  config. The env-var API key fallback is kept.

## [0.3.1] - 2026-10-14

Patch release. 1 PR (#72) since v0.3.0.

### Fixed

- **Session Model Sync**: fixed an issue where choosing a model through the
  TUI (`/model`, the `ModelSelector` via F2, and the `ProviderSelector` via F3) or when
  creating a new session was not synchronized to the session stored in the SQLite database.
  As a result the server's `effective_model` stayed locked to the session's default model
  (for example `"auto"`). It now calls `set_session_model` (`PUT /api/v1/sessions/{id}/model`)
  and synchronizes the session state consistently.
- **CLI `--max-steps` propagation**: fixed the CLI flag `maukode --max-steps <N>`, which
  used to be ignored and always fell back to the default of 50 steps in daemon mode
  and standalone TUI mode. The `--max-steps` value is now sent to the
  `POST /api/v1/agent/config` endpoint (`client.set_agent_config`) in daemon mode and
  passed on to `run_tui` in standalone mode.
- **Context Window Gateway Routing (`"auto"`)**: fixed context window resolution in the
  estimator for routing aliases such as `"auto"` (commonly used by gateways
  like LLMKita with 1M target models such as GLM-5.3/Claude/GPT) so it no longer
  falls back to the conservative 128k limit and resolves to 1,000,000 tokens (1M).

### Release Process

- Bumped the version to 0.3.1 in all manifests (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, both Cargo.lock files)

## [0.3.0] - 2026-10-13

Minor release. 1 PR (#69) since v0.2.1.

### Added — Post-Turn Reflection Loop

- **Reflection engine** (`crates/maukode-core/src/reflection.rs`): after a
  turn finishes, the agent evaluates its own performance and extracts durable
  lessons from the completed conversation.
- **`ReflectionConfig`**: opt-in via config with these parameters:
  - `enabled` — master switch (default: false)
  - `min_tool_calls` — the minimum number of tool calls in a turn before reflection
    triggers (default: 3), which avoids reflecting on trivial turns
  - `skill_creation` — lets the engine write new skill files to
    `.maukode/skills/learned/` (default: false)
- **Lesson extraction**: parsing LLM output for `[memory: title | taxonomy: X,
  content]` blocks, with taxonomy extraction (gotchas, conventions, etc.)
- **Skill proposal**: parsing `SKILL:` blocks into `ProposedSkill` with
  name, description, and prompt
- **SessionRunner integration**: the `with_reflection_config()` builder method
- **CLI flag**: `--reflection` to enable reflection in the TUI
- **Server API**: a `reflection` field in `AgentConfig` and `SetAgentConfigRequest`
  to configure reflection through the REST API
- **Desktop Settings**: a UI toggle for the reflection config on the Settings page
- **TS bindings**: `ReflectionConfig.ts` generated for the desktop app

### Changed

- `AgentConfig` no longer implements `Copy` (it gained the non-Copy
  `ReflectionConfig` field). Server handlers were updated to use
  `.clone()`.
- Workspace version bumped from 0.2.1 to 0.3.0

### Release Process

- Bumped the version to 0.3.0 in all manifests (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, both Cargo.lock files)

## [0.2.1] - 2026-10-13

Patch release. 1 PR (#66) since v0.2.0.

### Fixed

- **Compaction context window sizes**: replaced the hardcoded 200K-for-all-Claude and
  128K default with accurate per-model context window sizes, sourced
  from the official provider documentation:
  - **Claude**: the 5 families (opus/sonnet/haiku/fable/mythos) + Opus 4.7/4.8 → 1M;
    Opus 4/4.5, Sonnet 4/4.5, Haiku 3.5, 3.x → 200K
  - **GPT**: GPT-6/5.4/5.6 → 1,050,000; GPT-5/5.1 → 400,000; GPT-4o → 128,000;
    GPT-4 → 8,192; GPT-4-32k → 32,768; GPT-3.5 → 16,384
  - **GLM**: GLM-5.x → 1,000,000; GLM-4.x → 131,072
  - Pattern matching supports dashed (`claude-opus-4-8`) and dotted
    (`claude-opus-4.8`) notation

### Release Process

- Bumped the version to 0.2.1 in all manifests (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, both Cargo.lock files)
- The `v0.2.1` tag was previously created without a release commit — the version was still 0.2.0.
  This fix makes sure the binary reports the correct version.

## [0.2.0] - 2026-10-13

144 commits, 41 PRs (#18–#64) since v0.1.0. About 27,000 new lines of code.

### Added — Desktop App (Tauri + React)

- A native desktop app based on Tauri with the system WebView (`apps/desktop/`)
- A chat-style UI: a sidebar session list, a chat panel, a composer with context
  mentions, a token gauge, chips, and footer affordances
- Migration to Tailwind v4 + shadcn/ui (Radix UI + lucide icons)
- Session management: search, status filter, pinning, archive, rename, a per-session model
  switcher
- Command Palette and Shortcuts modal (keyboard-first navigation)
- A Settings page with Radix UI, plugin management, and the ChatGPT sign-in flow
- Review panel: per-file diffs, undo, worktree commit/merge/delete
- Interactive subagent cards with a slide-over trace drawer
- Native notifications and a repository picker
- External editor integration
- Project context (branch name, project info) in the session list and header
- Playwright e2e tests for the desktop UI

### Added — Memory & Knowledge Store

- The new `maukode-memory` crate with 7 modules:
  - Project-scoped knowledge store
  - FTS5 full-text search
  - Automatic memory extraction from conversation context
  - Memory links (relations between knowledge items)
  - Memory tools (exposing memory to the agent)
  - Scope & models
  - Dedicated error handling

### Added — Security & Sandboxing

- A runtime command sandbox with the modes `off`, `path_jail`, `bubblewrap`, and `auto`
- MCP server process sandboxing
- An API endpoint to change the sandbox mode at runtime
- Private tool review mode (gates access to likely secrets behind approval)
- A DNS-rebinding Host guard to protect the server

### Added — Subagent System

- SubagentTool implementation (child SessionRunner invocation)
- Subagent profiles: `codebase_researcher`, `security_auditor`
- MCP scoping & tool filtering for subagents
- A subagent registry (read-only) for discovery
- The server route `/api/v1/subagents` to list profiles
- Subagent events enter the global event feed

### Added — Skills Engine

- A backend skills engine with discovery from markdown files
- Server endpoints to list and manage skills
- The SkillDefinition protocol schema

### Added — Plugin System

- A backend plugin system: discovery, loading, management
- A plugin management API (enable/disable)
- Plugin MCP server config

### Added — Plans, Timeline & Prompt Queuing

- Structured plans: PlanStep, Plan, StepStatus, FinishReason
- Step-by-step timeline tracking
- Prompt queuing & steering (QueueSteeringPrompt)
- A live plan checklist widget in the TUI

### Added — Audit System

- An audit provenance viewer in the UI
- Permission provenance (records all permission decisions)
- Compaction provenance (records automatic compaction events)
- Persistent session turn identifiers
- SQLite tables for the permission and compaction audit

### Added — Preview System

- A sandboxed preview lifecycle API (#61): start/stop/refresh the preview dev server
- A live dev-server preview panel (#62): a reverse proxy for the preview web app
- The PreviewInfo response: status, port, URL, readiness, error

### Added — Worktree & Parallel Agents

- Parallel-agent management with git worktrees
- Worktree status: commit/merge/delete from the UI
- Per-session model (every session can use a different model)

### Added — ChatGPT Authentication

- Sign in with ChatGPT provider (#18)
- The ChatGPT auth flow (`chatgpt_auth.rs`, `chatgpt_client.rs`)
- Login lifecycle: login status, account management, use account

### Added — Server API

- SSE event replay with sequence numbers for reconnects
- Per-session permissions (permission mode per session, forkable)
- Session titles (set/get)
- A structured audit trail
- Parallel-agent API: worktree, per-session model, global feed
- A DNS-rebinding guard (Host header validation)
- Routes: auth, events, plugins, preview, settings, skills

### Added — CI & Release Infrastructure

- A Linux build workflow (`linux-build.yml`): a tarball release for x86_64,
  a smoke test under Xvfb, an artifact upload
- Desktop CI checks: compile + smoke-test the Tauri shell
- Self-hosted runner (#57)
- Blacksmith build cache
- TS bindings sync via `cargo test --features ts`

### Added — TUI Enhancements

- Markdown rendering with pulldown-cmark (`markdown.rs`)
- A render cache (`render_cache.rs`) to stop O(history) redraws
- Loading animations
- Turn cancellation (cancel generation mid-stream)
- Idle redraw skip (saves CPU)
- A subagent trace inspector modal
- A plan checklist widget

### Changed

- Workspace version bumped from 0.1.0 to 0.2.0
- The desktop app migrated from native HTML selects to Radix UI components
- The server event system uses sequence-numbered SSE replay

### Fixed

- The agent never ends a turn silently (a push model for finishing tasks)
- LLM stream tolerance: EOF, flaky SSE streams, retrying transient failures
- SQLite lock contention at first open (a flaky CI fix)
- Store cleanup: empty sessions, preventing a `read_file` panic (#53)
- A Send bound fix: box-pin the `start_turn` future to ensure `Send`
- Multiple desktop UI fixes: cropping, focus, sidebar toggle, localStorage
  guard, platform helpers
- PreviewRequest TypeScript type: added the missing `sandbox_mode` field (#63)
- Tauri Cargo.lock out of date for the `--locked` release build (#64)

## [0.1.0] - 2026-10-03

Initial release.
