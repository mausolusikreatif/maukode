# MauKode Architecture Deep Dive 🏗️

This document describes the internal subsystems, design patterns, and protocols that power **MauKode**.

```
                           ┌────────────────────────┐
                           │      Terminal UI       │ (Ratatui, crossterm)
                           │       or CLI REPL      │
                           └───────────┬────────────┘
                                       │ HTTP REST & SSE
                                       ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                      MauKode Daemon (maukode-server)                   │
 │                     Axum Server (http://127.0.0.1:4096)                │
 ├────────────────────────────────────────────────────────────────────────┤
 │                                                                        │
 │  ┌──────────────────────────────────────────────────────────────────┐  │
 │  │                  Session Runner (maukode-core)                   │  │
 │  │                                                                  │  │
 │  │   ┌────────────────────┐            ┌────────────────────────┐   │  │
 │  │   │  Context Manager   │            │   Multi-Turn Loop      │   │  │
 │  │   │  - AGENTS.md       │            │   - Streaming SSE      │   │  │
 │  │   │  - Tree-Sitter Map │            │   - Autonomous Drain   │   │  │
 │  │   │  - Compactor       │            │   - Shadow Checkpoint  │   │  │
 │  │   └─────────┬──────────┘            └───────────┬────────────┘   │  │
 │  │             │                                   │                │  │
 │  │             ▼                                   ▼                │  │
 │  │   ┌──────────────────────────────────────────────────────────┐   │  │
 │  │   │                      Tool Registry                       │   │  │
 │  │   │  - bash (with jail & risk assessment)                    │   │  │
 │  │   │  - read_file, write_file, edit_file, grep, glob          │   │  │
 │  │   │  - web_search & fetch_web_content                        │   │  │
 │  │   │  - get_codebase_map                                      │   │  │
 │  │   │  - External MCP Tools (via stdio JSON-RPC)               │   │  │
 │  │   └──────────────────────────────────────────────────────────┘   │  │
 │  └──────────────────────────────────┬───────────────────────────────┘  │
 │                                     │                                  │
 │                                     ▼                                  │
 │                     ┌───────────────────────────────┐                  │
 │                     │    SqliteSessionStore         │                  │
 │                     │    - messages                 │                  │
 │                     │    - messages_audit           │                  │
 │                     │    - sessions & metadata      │                  │
 │                     └───────────────────────────────┘                  │
 └────────────────────────────────────────────────────────────────────────┘
```

---

## 1. Decoupled Client-Server & Daemon Model

MauKode separates the user interface from agent execution:

- **Server Daemon (`maukode-server`)**:
  - Runs in the background as an Axum HTTP + Server-Sent Events (SSE) server.
  - Exposes REST endpoints for session lifecycle, model switching, permission approval, diffs, checkpoints, and compaction.
  - Automatically manages lifecycle via `~/.local/share/maukode/daemon.json` (storing PID, port, and start time).
  - Can be deployed remotely on a headless VPS with Bearer token authentication.
- **Client Interfaces (`maukode-tui` and `maukode-cli`)**:
  - Connect to the daemon over HTTP/SSE via `ServerClient`.
  - Multiple terminal sessions in different project directories can connect to a single daemon concurrently without resource contention.

---

## 2. Multi-Turn Autonomous Drain Loop

When a user submits a prompt, `maukode-core::runner::SessionRunner` executes an autonomous loop:

1. **Context Assembly**:
   - Gathers project instructions from `AGENTS.md` (or `MAUKODE.md` / `RULES.md`).
   - Injects the Tree-Sitter AST codebase skeleton map.
   - Evaluates context window usage and applies token pruning/compaction if necessary.
2. **Pre-Turn Shadow Checkpoint**:
   - Creates a shadow git checkpoint of the working directory before any mutating actions are taken.
3. **Streaming LLM Call**:
   - Streams text deltas and tool call arguments via Server-Sent Events (SSE) back to the user interface in real time.
4. **Tool Permission & Risk Assessment**:
   - Analyzes tool parameters against security policies.
   - If a command is high-risk (`rm -rf`, `sudo`, destructive git commands), execution pauses and yields a permission request event to the user.
5. **Tool Execution**:
   - Executes authorized tools within the sandboxed directory jail.
6. **Drain Iteration**:
   - Feeds tool outputs back into the conversation context and continues streaming the next model step until no further tool calls are requested.

---

## 3. Git Shadow Checkpoints & Rollback Engine

To guarantee safe autonomous operations, MauKode maintains an isolated shadow git storage located at `~/.local/share/maukode/checkpoints/<session_id>`:

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
   - Tracks exact characters and calculates token estimates using model-specific limits (Claude 200k, Gemini 1M, GPT-4o 128k, DeepSeek 64k).
2. **Older Tool Result Pruning**:
   - Truncates verbose stdout outputs (e.g. lengthy compilation logs or test outputs) in older turns to `prune_tool_results_chars`, while keeping the most recent turns completely intact.
3. **Compaction Engine (`/compact`)**:
   - Automatically triggered at $85\%$ context usage (or manually via `/compact`).
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

## 8. Subagents & Client Roadmaps

MauKode provides specialized documentation for multi-agent expansion and client interfaces:

- **[Subagent Architecture & Execution Model](./SUBAGENT_ARCHITECTURE.md)**: Details the subagent orchestration hub, child session runner, token budgeting, physical read-only sandboxing, and parent-child event streaming.
- **[Terminal UI Roadmap](./TUI_ROADMAP.md)**: Outlines Ratatui interface milestones, live plan timeline widgets, keyboard steering/queuing, session explorer upgrades, and subagent drill-down modals.
- **[Desktop Product Roadmap](./DESKTOP_ROADMAP.md)**: Covers the Tauri desktop app, session management, composer context mentions, and native desktop shell integrations.

