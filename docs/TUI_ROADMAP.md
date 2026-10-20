# Terminal UI (TUI) Roadmap & Subagent Experience 🖥️

> **Implementation status is tracked in [ROADMAP.md](./ROADMAP.md).** This document describes the
> direction and interaction design of the TUI; the short status labels below are kept in sync with
> that file.

This document details the roadmap and implementation phases for **MauKode TUI** (`crates/maukode-tui`), our terminal-native, keyboard-first developer interface built with [Ratatui](https://ratatui.rs) and [crossterm](https://github.com/crossterm-rs/crossterm).

---

## 1. Product Direction & Design Principles

MauKode TUI is a first-class client in the MauKode workspace, built for developers who live in tmux, SSH, and terminal windows.

### Core Principles

- **Near-Idle CPU & Zero Flicker**: Non-blocking asynchronous event loop (`tokio::sync::mpsc`) and
  differential rendering. While idle, the input thread polls every 100 ms and a 50 ms tick runs,
  but the screen is only redrawn when an event changes something or a turn is running.
- **High-Density Keyboard Ergonomics**: Every action is reachable via single-key hotkeys, vim-style navigation, or Telescope-style floating search modals.
- **Client-Server Transparency**: Connects seamlessly to the local background daemon (`http://127.0.0.1:4096`) or remote VPS instances with bearer token authentication.
- **Subagent & Plan Clarity**: Complex multi-step reasoning, plans, and subagent traces are rendered compactly with clean disclosure controls, avoiding terminal scrollback pollution.

---

## 2. Foundation: Current Capabilities (Phase 1 - Complete)

The current TUI implementation in `crates/maukode-tui` provides:

- **1-Line Telemetry Status Bar**: Displays daemon connection mode, session ID, active provider/model,
  and context token ratio.
- **High-Performance Chat Viewport**: Per-message render cache (`render_cache.rs`) prevents $O(N)$
  full-history re-parses during streaming bursts.
- **Markdown & Code Highlighting**: Syntax-highlighted code fences, inline code styles, and list
  formatting (`markdown.rs`).
- **Interactive Modals**: `/model`, `/provider`, `/sessions`, `/diff`, `/map`, `/mcp`, `/audit`,
  `/plan`, `/subagents`, and permission decisions (`[y]`/Enter allow once, `[a]` allow for the
  session, `[p]` run privately, `[n]`/Esc deny) are implemented.
- **Structured plans and subagents**: plan steps, in-stream subagent cards, and the child trace
  inspector are shipped; concurrent switching and split-pane layout remain planned.
- **Daemon integration**: resumable session events, checkpoints/undo, worktree review, provider/model
  selection, MCP status, and session audit are available.

The TUI does not yet provide web UI parity for steering/queueing, pin/archive session organization,
or `@file`/`#symbol` context autocomplete. Phase 2 and Phase 3 status is tracked below rather than
calling all daemon capabilities "integrated".

---

## 3. Phase 2: Daemon Feature Parity

Recent updates to `maukode-protocol` and `maukode-server` introduced powerful capabilities that are
integrated at different levels. Status below distinguishes the daemon, the web UI, and the TUI,
because a backend endpoint is not yet a terminal feature. See [ROADMAP.md](./ROADMAP.md) for the
evidence behind each label.

### Track 2.1: Live Plan Timeline Widget — **done in TUI**

- **Protocol Event**: the implementation consumes `LLMEvent::PlanCreated` and
  `LLMEvent::PlanStepUpdated`. The original `LLMEvent::PlanUpdate { steps }` name in the initial
  plan is not the current API and must not be used as a contract.
- **Layout & Rendering**: implemented as a collapsible floating header widget toggled with `F2` or
  `/plan`:
  ```text
  ┌─ Active Plan [3/5] ────────────────────────────────────────────────────────┐
  │ [✓] 1. Inspect AST repository map for authentication routes                 │
  │ [✓] 2. Locate race condition in token refresh flow                          │
  │ [◉] 3. Implement mutex synchronization in SessionRunner                    │
  │ [○] 4. Add regression test suite                                            │
  │ [○] 5. Verify GitHub CI                                                     │
  └────────────────────────────────────────────────────────────────────────────┘
  ```
- **Live State Updates**: real-time step status transitions (`Pending` ○, `InProgress` ◉,
  `Completed` ✓, `Failed` ✗) are rendered by `draw_plan_widget`.
- **Remaining work**: a dedicated side-panel layout on wide viewports and additional headless
  `TestBackend` coverage for failed-step and multi-plan transitions.

### Track 2.2: Mid-Turn Steering & Prompt Queuing — **web UI done, TUI planned**

The daemon exposes `/api/v1/sessions/{id}/steer` and queueing routes, and the web UI composer already
uses them. The TUI, however, still rejects submission while `App::is_running` is true, so this track
is unfinished terminal work, not a shipped feature. Planned TUI affordances:

- **Steering**: send mid-turn guidance through the existing daemon route without restarting the turn.
- **Prompt queuing**: queue follow-ups and show a badge such as `[Queued: 2 prompts waiting]`.
- **Interruption**: preserve the typed prompt while halting the running agent loop.
- Add terminal-event tests for all three behaviours.

### Track 2.3: Session Explorer Modal Overhaul (`/sessions`) — **basic TUI, planned expansion**

`ActiveModal::SessionSelector` currently provides a filtered session list for switching. It does not
yet implement the proposed `p`/`a` hotkeys, status filter chip cycling, or pinned/archived badges.
The web UI has related organization behavior, but its pin state is currently persisted
in client `localStorage`; the TUI must not treat that as shared daemon state.

### Track 2.4: Inline Context Autocomplete (`@file`, `#symbol`) — **web UI done, TUI planned**

The web UI composer has `@` suggestions for files, folders, symbols, and changes, with context
chips. There is no equivalent `@`/`#` popup picker or prompt-buffer injection in `crates/maukode-tui`.

---

## 4. Phase 3: The Subagent Terminal Phase — **mostly implemented**

Subagent cards, status/token display, and the trace inspector are implemented in the TUI. The
remaining work is concurrent child switching and the wide-terminal split-pane layout described below.

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

### 4.3 Concurrent Subagent Switcher — **planned**

The trace model can retain multiple child sessions, but number-key/`Tab` switching between concurrent
children is not implemented. Add that navigation and grouped parent-chat progress indicators.

### 4.4 Split-Pane Layout for Wide Terminals — **planned**

The current TUI does not automatically split terminals wider than 140 columns into parent and
subagent panes. Preserve the proposed 60/40 layout as a future interaction design.

---

## 5. Milestone Execution Plan

| Milestone | Deliverables | Verification |
| :--- | :--- | :--- |
| **M1: Plan Widget & Events** | **Done** — `PlanCreated`/`PlanStepUpdated` consumer, `/plan`, F2, and widget rendering | Add more `TestBackend` coverage for failed and repeated plans |
| **M2: Steering & Queueing** | **Backend/web UI only** — no TUI handlers or badge | Terminal key/event integration tests |
| **M3: Session Explorer Modal** | **Basic only** — filtered TUI selector; no pin/archive/filter chips | Modal hotkey and state integration tests |
| **M4: Subagent Stream Widgets** | **Done** — cards and `Subagent*` event reduction/rendering | Expand SSE simulation coverage and cancellation UX |
| **M5: Subagent Trace Modal** | **Done** — `ActiveModal::SubagentViewer`, scrolling, and virtual-terminal coverage | Add concurrent-child switching |
| **M6: Context Autocomplete** | **Web UI only** — TUI picker/insertion flow is absent | Fuzzy completion snapshot tests |
