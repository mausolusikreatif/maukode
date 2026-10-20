# MauKode Architecture Deep Dive 🏗️

This document describes the internal subsystems, design patterns, and protocols that power **MauKode**.

```
      ┌──────────────────────┐   ┌──────────────────────┐   ┌──────────────────────┐
      │     Terminal UI      │   │       CLI REPL       │   │   Web UI (browser)   │
      │  (ratatui/crossterm) │   │  `maukode --cli/run` │   │     apps/desktop     │
      └──────────┬───────────┘   └──────────┬───────────┘   └──────────┬───────────┘
                 └──────────────────────────┼──────────────────────────┘
                                            │ HTTP REST & SSE
                                            ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                      MauKode Daemon (maukode-server)                   │
 │                     Axum server (http://127.0.0.1:4096)                │
 ├────────────────────────────────────────────────────────────────────────┤
 │                                                                        │
 │  ┌──────────────────────────────────────────────────────────────────┐  │
 │  │                  Session Runner (maukode-core)                   │  │
 │  │                                                                  │  │
 │  │   ┌────────────────────┐            ┌────────────────────────┐   │  │
 │  │   │  Context Manager   │            │   Multi-step loop      │   │  │
 │  │   │  - AGENTS.md       │            │   - Streaming LLM      │   │  │
 │  │   │  - Tree-Sitter map │            │   - Permissions        │   │  │
 │  │   │  - Knowledge recall│            │   - Shadow checkpoint  │   │  │
 │  │   │  - Compactor       │            │   - Reflection         │   │  │
 │  │   └─────────┬──────────┘            └───────────┬────────────┘   │  │
 │  │             ▼                                   ▼                │  │
 │  │   ┌──────────────────────────────────────────────────────────┐   │  │
 │  │   │                      Tool Registry                       │   │  │
 │  │   │  - read_file, write_file, edit_file, grep, glob (jail)   │   │  │
 │  │   │  - bash → ExecBackend: host | bubblewrap | lawang cell   │   │  │
 │  │   │  - web_search, fetch_web_content, get_codebase_map       │   │  │
 │  │   │  - memory_* (maukode-memory), invoke_subagent            │   │  │
 │  │   │  - external MCP tools (stdio JSON-RPC)                   │   │  │
 │  │   └──────────────────────────────────────────────────────────┘   │  │
 │  └──────────────────────────────────┬───────────────────────────────┘  │
 │                                     ▼                                  │
 │                     ┌───────────────────────────────┐                  │
 │                     │   SQLite (maukode.db)         │                  │
 │                     │   - sessions, messages        │                  │
 │                     │   - messages_audit, audits    │                  │
 │                     │   - knowledge store (memory)  │                  │
 │                     └───────────────────────────────┘                  │
 └────────────────────────────────────────────────────────────────────────┘
```

For what is implemented and what is planned, see [ROADMAP.md](./ROADMAP.md).

---

## 1. Decoupled Client-Server & Daemon Model

MauKode separates the user interface from agent execution:

- **Server Daemon (`maukode-server`)**:
  - Runs in the background as an Axum HTTP + Server-Sent Events (SSE) server.
  - Exposes REST endpoints for session lifecycle, model switching, permission approval, diffs, checkpoints, and compaction.
  - Automatically manages lifecycle via `~/.local/share/maukode/daemon.json` (storing the URL, PID, and token).
  - Can be deployed remotely on a headless VPS with Bearer token authentication.
- **Client Interfaces**:
  - `maukode-tui` and `maukode-cli` connect to the daemon over HTTP/SSE via `ServerClient`;
    `--standalone` runs the agent in-process instead.
  - The web UI (`apps/desktop`) is a static bundle served by the daemon (`--web-dir`) and talks to
    the same API (see [API.md](./API.md)).
  - Several clients, in different project directories, can use one daemon at the same time.
    Session events are kept in a replay log so a client can reconnect and resume a stream.

---

## 2. Multi-Turn Autonomous Drain Loop

When a user submits a prompt, `maukode-core::runner::SessionRunner` executes an autonomous loop:

1. **Context Assembly**:
   - Gathers project instructions from `AGENTS.md` (or `MAUKODE.md` / `RULES.md`).
   - Injects the Tree-Sitter AST codebase skeleton map and knowledge recalled for the prompt.
   - Evaluates context window usage and applies token pruning/compaction if necessary.
   - In the daemon, the tool registry, MCP tools, and context manager are currently built anew for
     every turn (see the backlog in [ROADMAP.md](./ROADMAP.md)).
2. **Pre-Turn Shadow Checkpoint**:
   - Creates a shadow git checkpoint of the working directory before any mutating actions are taken.
3. **Streaming LLM Call**:
   - Streams text deltas and tool call arguments via Server-Sent Events (SSE) back to the user interface in real time.
4. **Tool Permission & Risk Assessment**:
   - Applies the permission mode. In `ask` mode, `bash` commands flagged by the pattern-based risk
     assessment (`rm -rf`, `sudo`, destructive git, secret access, …) pause the turn and emit a
     permission request; `strict` and `review` ask for every tool call; `trust` never asks.
5. **Tool Execution**:
   - Runs the approved tools one after another. File tools are confined by the path jail; `bash`
     runs through the configured `ExecBackend` (host process, bubblewrap, or a lawang cell).
6. **Drain Iteration**:
   - Feeds tool outputs back into the conversation context and continues streaming the next model
     step until no further tool calls are requested.
7. **End of Turn**:
   - On a clean finish: optional memory extraction, reflection, and memory consolidation.

---

## 3. Git Shadow Checkpoints & Rollback Engine

To guarantee safe autonomous operations, MauKode maintains an isolated shadow git storage located at `~/.local/share/maukode/shadow_git/<hash of the workspace path>`:

- **Zero Intrusion on Main Git**: Checkpoint commits are recorded in the shadow repository; the user's primary `.git` history is never touched or polluted.
- **Pre-Turn Snapshots**: Every turn that may execute tools automatically commits a tree snapshot.
- **Visual Diffs (`/diff`)**: Compares the current workspace state with the baseline checkpoint, generating unified colored diffs.
- **Instant Rollback (`/undo`)**: Copies the clean state from the selected checkpoint back to the workspace, seamlessly reversing unwanted agent modifications.

---

## 4. Tree-Sitter AST Codebase Map

MauKode features an intelligent code indexer (`maukode-core::repo_map`):

- **Grammar Support**: Rust, TypeScript/JavaScript, Python, and Go via tree-sitter grammars.
- **Definition & Reference Extraction**: Extracts structs, classes, interfaces, traits, functions, methods, and call references.
- **PageRank Centrality**: Constructs a directed symbol graph where incoming calls boost the centrality score of key architectural components.
- **Token-Budget Skeleton**: Formats an ASCII tree of the most significant symbols within a strict token budget (default 800 tokens), auto-injected into the system prompt.
- **Viewer (`/map`)**: Interactive fullscreen scrollable modal displaying the workspace's symbol hierarchy.

---

## 5. Model Context Protocol (MCP) Client

MauKode integrates the Anthropic Model Context Protocol (MCP):

- **Transport**: Standard I/O (stdio) communication with child processes using JSON-RPC 2.0.
- **Protocol Handshake**: Implements `initialize`, `notifications/initialized`, `tools/list`, and `tools/call`.
- **Dynamic Tool Bridging**: External tools exposed by MCP servers are converted on the fly into `maukode-tools::Tool` instances and registered in the active `ToolRegistry`.
- **Process Management**: Child processes are managed asynchronously with graceful termination on session shutdown or drop.

---

## 6. Context Pruning & Conversation Compactor

Long agentic sessions can rapidly exceed model context limits. MauKode manages context via a three-tiered pipeline:

1. **Context Estimation**:
   - Estimates tokens with a ~4 characters per token heuristic and compares them with the model's
     context window from `TokenEstimator::context_window_for_model` (for example 1M for Claude 5
     and Gemini, 200k for older Claude models, 128k as the fallback).
2. **Older Tool Result Pruning**:
   - Truncates verbose tool outputs in older turns to 2,000 characters (head and tail kept), and
     caps even recent outputs at 30,000 characters (`ContextPruner`). The stored history is not
     modified.
3. **Compaction Engine (`/compact`)**:
   - Triggered automatically at 85% context usage (a warning is shown from 75%), or manually via
     `/compact`.
   - Extracts key tasks, modified files, errors resolved, and current state.
   - Replaces older message turns with a structured compaction summary.
4. **Audit Preservation**:
   - Raw, uncompacted messages are permanently preserved in the SQLite `messages_audit` table for full historical inspection.

---

## 7. Web Search & Documentation Fetcher

To bridge knowledge gaps beyond training cutoff dates, MauKode equips agents with:

- **`WebSearchTool` (`web_search`)**:
  - Out-of-the-box DuckDuckGo HTML parser requiring zero credentials or configuration.
  - Pluggable support for Brave Search API, Tavily, and SearXNG.
  - Extracts clean target URLs and snippet summaries.
- **`FetchWebContentTool` (`fetch_web_content`)**:
  - Downloads web pages with a 15-second timeout and 2MB limit.
  - Parses raw HTML into token-efficient Markdown, stripping headers, footers, scripts, styles, and advertisements.
- **Runtime Aliasing**:
  - `ToolRegistry` resolves common LLM aliases (`search_web` $\to$ `web_search`, `fetch_url` / `read_url` $\to$ `fetch_web_content`) without duplicating tool definitions in the system prompt.

---

## 8. Shell Execution Backends

The `bash` tool runs through an `ExecBackend`, chosen per session from `config.json`:

- **Host** (`execution.backend = "host"`, default): a local process in the workspace. With
  `security.sandbox_mode = "bubblewrap"` (or `"auto"` when `bwrap` is installed) it runs inside
  bubblewrap on Linux; with the default `"path_jail"` it is an ordinary host process.
- **lawang** (`execution.backend = "lawang"`): an isolated cell of a separate lawang daemon over
  HTTP, at `process` (bubblewrap), `sandbox` (gVisor), or `vm` (Firecracker) isolation. One cell per
  session, mounted at `/work`; file tools keep working on the host folder and are wrapped in
  `SyncedTool` so both sides see each other's changes.

The path jail only validates file-tool paths. It is not an operating-system sandbox and does not
restrict `bash` on the host backend.

---

## 9. Related Documents

- **[Roadmap and Feature Status](./ROADMAP.md)**: what is implemented, with evidence, and the backlog.
- **[Memory Architecture](./MEMORY_ARCHITECTURE.md)**: the project knowledge store and `memory_*` tools.
- **[Subagent Architecture & Execution Model](./SUBAGENT_ARCHITECTURE.md)**: child session runner,
  token budgeting, read-only mode, and parent-child event streaming.
- **[Terminal UI Roadmap](./TUI_ROADMAP.md)**: TUI direction and interaction design.
- **[Web UI Roadmap](./WEB_UI_ROADMAP.md)**: web UI direction and interaction design.
- **[API](./API.md)**: HTTP and SSE endpoints of the daemon.
