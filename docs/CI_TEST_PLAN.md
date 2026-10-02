# CI test plan

## Current baseline (2026-10-03)

The `Rust CI` workflow runs `cargo test --workspace --locked` and Clippy on
`ubuntu-latest` for every push and pull request. All 56 current tests pass:

| Crate | Tests | Current coverage |
| --- | ---: | --- |
| `maukode-core` | 30 | Compaction, memory discovery, repository map parsing, MCP mock handshake, checkpoint and audit retention, local OAuth callback/token/JWKS/refresh/logout fixtures and credential locking, Responses SSE errors and tool replay |
| `maukode-tools` | 10 | Command risk classification, basic path jail, registry aliases and unknown names, HTML parsing |
| `maukode-server` | 3 | Health/auth and session creation, unknown permission reply, selected read endpoints |
| `maukode-tui` | 9 | Chat wrap/scroll, input and dialog wrap, event-to-render path, tool-call completion, streaming smoke budget |
| `maukode-llm` | 4 | OpenAI-compatible fragmented names/arguments, multiple calls, invalid arguments, local SSE fixture |
| `maukode-protocol`, `maukode-cli` | 0 | No direct tests |

The server tests bind ephemeral loopback ports before starting the router, so
they do not depend on fixed ports or startup sleeps. `ServerState` and the TUI
construct config/database paths from the user's XDG directories. Tests should
isolate these paths to avoid shared state. No test should require a live LLM
account.

The audit found preexisting formatting drift in
`crates/maukode-core/src/compaction/estimator.rs` and
`crates/maukode-core/src/store/sqlite.rs`. The companion tool-call fix formats
those files so `cargo fmt --all --check` can become a required CI check.

Code review also found three concrete cases to capture in tests. The
OpenAI-compatible adapter previously took only the first item after collecting
tool results; the companion fix now emits all of them and tests fragmented
tool names and arguments. SQLite builds
a generated title with a byte slice (`&text[..40]`), which can panic when the
boundary cuts a multibyte character. The Anthropic adapter accumulates usage
but does not appear to emit an `LLMEvent::Usage`. These are code-level findings,
not failures reproduced by the current suite.

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

1. **Every PR:** formatting check (after current formatting is fixed), `cargo test --workspace --locked`, `cargo clippy --all-targets --all-features -- -D warnings`, and a locked workspace build. Tests above use fake clients, fixture HTTP/SSE servers, `TestBackend`, and temporary directories.
2. **Scheduled or release CI:** a platform matrix for Linux/macOS/Windows where supported, the documented minimum Rust version, and larger TUI/stream/SQLite performance workloads. Use dedicated, stable benchmark hardware for tight latency limits; the current wall-clock TUI smoke limit is only a regression guard.
3. **Manual opt-in:** live-provider contract smoke tests with secrets. Do not make external API availability, quotas, or public websites a PR gate.

The first implementation batch should cover the LLM SSE adapters, runner and
permission flow, and file/shell jail. These paths connect untrusted streamed
data to filesystem and command execution, yet currently have no direct
end-to-end tests.
