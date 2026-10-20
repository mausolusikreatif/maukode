# CI test plan

All builds and tests run in GitHub CI (`.github/workflows/ci.yml`); agents must not run them
locally (see `AGENTS.md`).

## Current baseline (2026-10-20)

The `Rust CI` workflow runs on a self-hosted runner (`[self-hosted, rekarta]`) for every push to
`master` and every pull request:

1. `cargo fmt --all --check`
2. `cargo test --workspace --locked`
3. TypeScript bindings check: `export_bindings` tests with the `ts` features, then a check that
   `apps/desktop/src/api/generated` has no diff.
4. `cargo clippy --all-targets --all-features -- -D warnings`

The web UI job (`npm ci`, `npm run typecheck`, `npm test` in `apps/desktop`) only runs on a manual
`workflow_dispatch` with `run_desktop` enabled. `linux-build.yml` builds the release tarball on
`v*` tags and `claude/**` branches.

Test functions per crate (counted from `#[test]`/`#[tokio::test]` attributes):

| Crate | Tests | Main coverage |
| --- | ---: | --- |
| `maukode-core` | 80 | Runner turns with a scripted LLM, compaction, repo map, MCP handshake, checkpoints, ChatGPT auth fixtures, SQLite store |
| `maukode-tools` | 41 | Bash risk classification, path jail, registry, web parsing, exec backends (`tests/lawang_e2e.rs`) |
| `maukode-server` | 40 | Auth, sessions, SSE, settings, preview, exec backend (`tests/`) |
| `maukode-memory` | 23 | Knowledge store, links, consolidation |
| `maukode-tui` | 20 | Chat wrap/scroll, dialogs, event-to-render path, streaming smoke budget |
| `maukode-llm` | 10 | OpenAI-compatible SSE fragments, multiple tool calls, EOF after finish, `stream_options` retry |
| `maukode-protocol`, `maukode-cli` | 0 | No direct tests |

The web UI has unit tests for the SSE parser, thread reducer, session filters, diff parser,
notifications, and composer context (`*.test.ts`).

No test may require a live LLM account or the public internet. Server tests bind ephemeral loopback
ports and isolate config and data with `MAUKODE_HOME`.

### TUI tests

The TUI tests use Ratatui's in-memory `TestBackend` and deterministic chat chunks; they need no
terminal, API key, LLM, or fake HTTP server. They cover word wrapping in chat, the multi-row input
and dialogs, top and bottom of a 2,000-message scrollback, and burst streaming (4,000 chunks with
40 redraws). An end-to-end TUI test sends keyboard and LLM events through the app, then inspects a
full virtual terminal at narrow and wide sizes, including permission-command scrolling and provider
URL entry. Generous time limits in the stream tests catch severe regressions; use a dedicated
benchmark runner for precise latency comparisons.

The viewport uses Ratatui's own wrapped-line count to calculate scroll limits. The `Paragraph`
scroll API takes a `u16` row offset, so histories beyond 65,535 visual rows need a separate
windowing implementation to reach the newest rows reliably.

### Known gaps found by code review

- The Anthropic adapter accumulates usage but does not emit `LLMEvent::Usage`, and it has no SSE
  fixture tests.
- Bash approval bypasses (`bash -c`, prefix-matched `bash_whitelist`) have no tests; see the
  security backlog in [ROADMAP.md](./ROADMAP.md).

## P0: required before merging behavior changes

| Area | Deterministic test cases to add | Main failure caught |
| --- | --- | --- |
| LLM adapters (`maukode-llm`) | Extend local SSE fixture tests for OpenAI-compatible and Anthropic requests. Assert headers, system/user/assistant/tool payloads; text and reasoning deltas; fragmented and interleaved tool names/arguments; **multiple tool calls**; finish reasons; token usage; malformed event, HTTP error, and dropped connection. OpenAI parser unit tests and one local HTTP/SSE fixture now cover fragmented names; the rest remains. | Lost or reordered stream events and incompatible provider payloads. |
| Runner (`maukode-core/src/runner.rs`) | Inject a fake `LLMClient` and fake `Tool`: plain reply, tool call then continuation, two tool calls in one turn, tool failure, stream error, max-step stop, and session state/message/event sequence. | Agent hangs, drops tool calls, or persists incorrect turn state. |
| Permission path (`maukode-core/src/permission.rs` and runner) | For Ask/Strict/Trust, assert safe command, warning/danger command, AllowOnce, AllowSession, Deny, whitelist scope, and closed decision channel. Use paused Tokio time for the timeout path. | A denied command executes or a required approval is skipped. |
| File/shell safety (`maukode-tools`) | With `tempdir`, exercise read/write/edit/glob/grep and bash success, error, timeout. Assert jailed `..`, absolute paths, existing symlinks, symlink parents of new files, and allowed external paths cannot escape policy. Verify exact-once edit semantics. | Workspace escape, unintended file edits, or hung child process. |
| Server/client (`maukode-server`) | Use an in-process router or ephemeral listener and isolated store/config. Assert protected routes reject missing/wrong Bearer tokens; create/list/get session; submit prompt with fake provider; receive SSE events and finish; concurrent prompt returns conflict; valid/invalid permission response; provider/model endpoints and errors. | API contract regressions, dropped events, auth bypass, or duplicate turns. |
| SQLite (`maukode-core/src/store/sqlite.rs`) | Round trip all message/part/state variants and Unicode titles; repeated save, compaction/audit idempotency, directory filtering, latest-session ordering, reopen database, missing/corrupt rows. | Lost history, incorrect resume, or Unicode panic. |
| Protocol (`maukode-protocol`) | JSON round trips for every `LLMEvent`, `ContentPart`, `SessionState`, and permission decision, including unknown/invalid variants where rejection is expected. | Client/server wire format drift. |

## P1: required when the corresponding area changes

| Area | Test cases to add |
| --- | --- |
| TUI | Resize matrix (narrow, normal, wide); Unicode and long unbroken input; cursor move/delete at wrap boundaries; all modal types; stream while manually scrolled; histories crossing the `u16` paragraph-scroll boundary. Keep visual assertions deterministic with `TestBackend`. |
| Config and CLI | Isolated XDG config defaults, OpenCode import, provider/model switching, invalid config, `--help`/invalid arguments, `init` without and with `--force`, standalone/remote mode selection. Execute the CLI binary only for process-level contracts. |
| MCP | Mock stdio server for response ID matching, concurrent requests, timeout, malformed JSON, child exit, tool error, shutdown, and config precedence. Existing test covers only the happy handshake/tools path. |
| Checkpoint/undo | Multiple turns and files, deleted/renamed files, clean state, invalid undo target, path with spaces, and checkpoint persistence/recovery. Existing test covers one lifecycle. |
| Context/memory/repo map | Rule precedence and duplicates, ignored files/limits, AST parse failures, token-budget truncation, and supported language fixtures. |
| Web tools | Local HTTP fixtures for HTML/JSON/plain text, redirects, HTTP errors, over-size body, UTF-8 truncation, and search-provider response parsing. No public internet requests in PR CI. |
| Daemon | Isolated XDG directory and ephemeral port for start/status/stale PID/stop/restart; verify cleanup of child process and daemon file. |

## CI execution policy

1. **Every PR:** formatting check, `cargo test --workspace --locked`, `cargo clippy --all-targets --all-features -- -D warnings`, and a locked workspace build. Tests above use fake clients, fixture HTTP/SSE servers, `TestBackend`, and temporary directories.
2. **Scheduled or release CI:** a platform matrix for Linux/macOS/Windows where supported, the documented minimum Rust version, and larger TUI/stream/SQLite performance workloads. Use dedicated, stable benchmark hardware for tight latency limits; the current wall-clock TUI smoke limit is only a regression guard.
3. **Manual opt-in:** live-provider contract smoke tests with secrets. Do not make external API availability, quotas, or public websites a PR gate.

The first implementation batch should cover the LLM SSE adapters, runner and
permission flow, and file/shell jail. These paths connect untrusted streamed
data to filesystem and command execution, yet currently have no direct
end-to-end tests.
