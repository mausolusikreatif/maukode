# Terminal UI (TUI) Roadmap & Subagent Experience 🖥️

This document details the roadmap and implementation phases for **MauKode TUI** (`crates/maukode-tui`), our terminal-native, keyboard-first developer interface built with [Ratatui](https://ratatui.rs) and [crossterm](https://github.com/crossterm-rs/crossterm).

---

## 1. Product Direction & Design Principles

MauKode TUI is a first-class client in the MauKode workspace, built for developers who live in tmux, SSH, and terminal windows.

### Core Principles

- **0% Idle CPU & Zero Flicker**: Non-blocking asynchronous event loop (`tokio::sync::mpsc`) and intelligent differential rendering. Redraws occur exclusively when events arrive.
- **High-Density Keyboard Ergonomics**: Every action is reachable via single-key hotkeys, vim-style navigation, or Telescope-style floating search modals.
- **Client-Server Transparency**: Connects seamlessly to the local background daemon (`http://127.0.0.1:4096`) or remote VPS instances with bearer token authentication.
- **Subagent & Plan Clarity**: Complex multi-step reasoning, plans, and subagent traces are rendered compactly with clean disclosure controls, avoiding terminal scrollback pollution.

---

## 2. Foundation: Current Capabilities (Phase 1 - Complete)

The current TUI implementation in `crates/maukode-tui` provides:

- **1-Line Telemetry Status Bar**: Displays daemon connection mode (`● daemon:4096` / `● remote:vps`), session ID, active provider/model, and context token ratio.
- **High-Performance Chat Viewport**: Per-message render cache (`render_cache.rs`) preventing $O(N)$ full-history re-parses during streaming bursts.
- **Markdown & Code Highlighting**: Syntax-highlighted code fences, inline code styles, and list formatting (`markdown.rs`).
- **Interactive Modals**:
  - `/model`: Fuzzy-filterable model selector.
  - `/provider`: Provider manager & switcher (Anthropic, OpenAI, OpenRouter, Ollama, ChatGPT subscription).
  - `/sessions`: Basic session switching modal.
  - `/diff`: Interactive color-coded unified git diff viewer.
  - `/map`: Tree-Sitter AST symbol centrality skeleton map viewer.
  - `/mcp`: Model Context Protocol server status viewer.
  - `/audit`: Full session SQLite audit log viewer.
  - Permission modals for high-risk actions (`[a]` allow once, `[s]` allow session, `[p]` run privately, `[d]` deny).

---

## 3. Phase 2: Daemon Feature Parity

Recent updates to `maukode-protocol` and `maukode-server` introduced powerful capabilities that are now being integrated into the TUI:

### Track 2.1: Live Plan Timeline Widget

- **Protocol Event**: Reactive handling of `LLMEvent::PlanUpdate { steps }`.
- **Layout & Rendering**:
  - A collapsible side-panel on wider viewports or a floating header widget toggled with `F2` or `/plan`:
  ```text
  ┌─ Active Plan [3/5] ────────────────────────────────────────────────────────┐
  │ [✓] 1. Inspect AST repository map for authentication routes                 │
  │ [✓] 2. Locate race condition in token refresh flow                          │
  │ [◉] 3. Implement mutex synchronization in SessionRunner                    │
  │ [○] 4. Add regression test suite                                            │
  │ [○] 5. Verify GitHub CI                                                     │
  └────────────────────────────────────────────────────────────────────────────┘
  ```
- **Live State Updates**: Real-time step status transitions (`Pending` ○, `InProgress` ◉, `Completed` ✓, `Failed` ✗).

### Track 2.2: Mid-Turn Steering & Prompt Queuing

Currently, input is locked while a turn is executing. Phase 2 unlocks full in-flight steering and prompt queuing:

- **Steering Affordance (`Ctrl+S` / `Enter` on active turn)**:
  - Sends user input as immediate mid-turn steering via `POST /api/sessions/{id}/steer`.
  - Injects guidance into the agent's next tool boundary without restarting the turn.
- **Prompt Queuing (`Ctrl+Q` / `Tab` to queue)**:
  - Saves the entered prompt into the session queue via `POST /api/sessions/{id}/queue`.
  - Visual badge in prompt bar: `[Queued: 2 prompts waiting]`.
  - Queued prompts automatically execute in sequence as turns complete.
- **Interruption (`Ctrl+C`)**:
  - Preserves typed prompt text while cleanly halting the running agent loop.

### Track 2.3: Session Explorer Modal Overhaul (`/sessions`)

Upgrades `ActiveModal::SessionSelector` to support the full server session query API:

- **Keyboard Toggles**:
  - `p`: Toggle pin / unpin on the selected session (`PATCH /api/sessions/{id}`).
  - `a`: Toggle archive / restore on the selected session.
  - `Tab`: Cycle status filter chips (`[All]`, `[Active]`, `[Pinned]`, `[Archived]`).
- **Real-Time Fuzzy Filter**: Instant substring filtering on session titles and project paths.
- **Visual Badges**: Clear visual markers (`📌 Pinned`, `📦 Archived`, `● Running`, `⚠️ Needs Approval`).

### Track 2.4: Inline Context Autocomplete (`@file`, `#symbol`)

- Typing `@` opens an in-viewport fuzzy file picker overlay (searching workspace files via cached git tree).
- Typing `#` opens a symbol search overlay (leveraging AST symbols parsed by `maukode-core::repo_map`).
- Selected references are cleanly injected into the prompt buffer.

---

## 4. Phase 3: The Subagent Terminal Phase

When specialized subagents (such as read-only Codebase Researchers or Security Auditors) are deployed by the parent agent, the TUI must provide crystal-clear visibility without overwhelming the terminal screen.

### 4.1 In-Stream Subagent Cards

In the main chat viewport, a running subagent is represented as a high-density, dynamic widget:

```text
┌─ 🤖 Subagent: Codebase Researcher (read-only) ──────────────────────────────┐
│ Goal: Locate all usages of TokenBucket across crates                        │
│ Status: ◉ Running step 4 • grep "TokenBucket" in crates/maukode-core/       │
│ Tokens: 3,420 / 32,000 budget                                               │
│ [Enter] Inspect Trace    [Ctrl+C] Cancel Subagent                           │
└─────────────────────────────────────────────────────────────────────────────┘
```

Upon completion, the card collapses into a concise summary with a green completion checkmark, showing elapsed time and token consumption.

### 4.2 Drill-Down Subagent Trace Modal (`ActiveModal::SubagentViewer`)

Pressing `Enter` on any subagent card (or executing `/subagents`) opens a dedicated fullscreen or floating drill-down modal:

```text
┌─ Subagent Trace: Codebase Researcher ────────────────────────── [Tab: Agent 1/2] ─┐
│                                                                                   │
│ 10:14:02  [Goal] "Search crates/maukode-core for TokenBucket mutex locks"        │
│ 10:14:03  [Thought] Let's first search for TokenBucket definitions with grep.    │
│ 10:14:04  [Tool Call] grep(query="struct TokenBucket")                           │
│ 10:14:04  [Tool Output] crates/maukode-core/src/rate_limit.rs:14:pub struct...    │
│ 10:14:05  [Tool Call] read_file(path="crates/maukode-core/src/rate_limit.rs")    │
│ 10:14:06  [Synthesis] Found TokenBucket defined in rate_limit.rs; protected by   │
│           Arc<tokio::sync::Mutex<TokenBucket>> in runner.rs:104.                 │
│                                                                                   │
│ [↑/↓] Scroll    [Tab] Next Subagent    [Esc] Back to Parent Chat                  │
└───────────────────────────────────────────────────────────────────────────────────┘
```

### 4.3 Concurrent Subagent Switcher

When multiple subagents are spawned concurrently:
- Number keys `1`–`9` or `Tab` immediately switch the trace inspector to that subagent.
- The parent chat displays a grouped progress bar: `Subagents: [1: ✓] [2: ◉] [3: ○]`.

### 4.4 Split-Pane Layout for Wide Terminals

On terminals with width $\ge 140$ columns:
- Automatic or manual (`Ctrl+B`) split mode:
  - **Left Pane (60%)**: Parent conversation, plan checklist, and user prompt.
  - **Right Pane (40%)**: Live subagent activity stream and tool traces.

---

## 5. Milestone Execution Plan

| Milestone | Deliverables | Verification |
| :--- | :--- | :--- |
| **M1: Plan Widget & Events** | `PlanTimeline` Ratatui component, `LLMEvent::PlanUpdate` consumer | Headless Ratatui unit tests (`TestBackend`) |
| **M2: Steering & Queueing** | `Ctrl+S` / `Ctrl+Q` key handlers, prompt queue badge, server API calls | Terminal event integration tests |
| **M3: Session Explorer Modal** | Pin/archive hotkeys, filter chip tabs, fuzzy query input | Modal keyboard interaction tests |
| **M4: Subagent Stream Widgets** | In-stream subagent cards, `LLMEvent::Subagent*` events | SSE event simulation tests |
| **M5: Subagent Trace Modal** | Interactive child transcript inspector, subagent switching | Full virtual terminal snapshot tests |
| **M6: Context Autocomplete** | `@file` and `#symbol` popup pickers in input textarea | Fuzzy completion snapshot tests |
