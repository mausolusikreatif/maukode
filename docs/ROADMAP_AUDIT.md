# Roadmap Audit

Audited against `master` at `434e12c` on October 7, 2026. This is a code-evidence snapshot,
not a replacement for the product direction in `DESKTOP_ROADMAP.md` and `TUI_ROADMAP.md`.

## Executive summary

Both roadmaps are useful design documents, but neither is a reliable implementation tracker. The
desktop roadmap describes several shipped features as future work, while the TUI milestone table has
not been updated after M1, M4, and M5 landed. Keep product rationale in those documents and track
status in one checklist (this file or issues) to avoid implementing the same feature twice.

## Desktop roadmap

### Shipped or substantially shipped

- Project/session dashboard, pin/archive state, grouping, and session filtering.
- Native directory picker and recent-repository selection in `NewSessionDialog.tsx`.
- Worktree creation plus commit, merge, and discard flows.
- Resumable session SSE, permission decisions, checkpoints/undo, and per-file review.
- Plan timeline and live plan-step state.
- In-flight steering and follow-up queueing in the server and desktop composer.
- Command palette and keyboard shortcuts.
- Desktop completion notifications.
- Subagent cards and trace drawer.
- Backend skills discovery and backend plugin discovery/enable/disable.

### Partial

- Skills exist in the backend, but the roadmap's complete desktop capability browser, authoring flow,
  and composer autocomplete are not present.
- MCP is implemented and scoped, but unified MCP/skill/plugin management in the desktop is incomplete.
- Session organization uses client-side metadata in places; it is not yet a fully persisted server-side
  project model.
- Native integration has a folder picker and daemon lifecycle, but menus/tray, deep links, updater,
  release channels, editor launch actions, and polished crash recovery remain roadmap work.

### Not evidenced in the current tree

- Rich `@file`/symbol/diff context chips and a context-budget inspector.
- Inline diff annotations and a structured review summary.
- Scheduled/background automations with history and retry policy.
- A restricted browser/computer-use runtime.
- The stronger isolated execution architecture described in the security section.

### Documentation correction needed

Sections that say the directory must be entered manually and that steering/queueing must be added are
stale. Future edits should label each requirement as **done**, **partial**, or **planned**, rather than
mixing current-state prose with unmarked proposals.

## TUI roadmap

| Milestone | Audited status | Evidence / remaining work |
|---|---|---|
| M1 Plan widget & events | **Done** | `active_plan`, `PlanCreated`, `PlanStepUpdated`, `/plan`, F2, and `draw_plan_widget` exist. The roadmap's single `PlanUpdate` event name is stale. |
| M2 Steering & queueing | **Backend only** | Server routes/state and desktop controls exist; no TUI key handlers or queue badge were found. |
| M3 Session explorer | **Not started in TUI** | Basic `/sessions` exists, but no TUI pin/archive/filter-chip implementation was found. |
| M4 Subagent stream widgets | **Done** | Spawn/progress/finish events are reduced into in-stream subagent state and rendered. |
| M5 Subagent trace modal | **Done** | The trace inspector and virtual-terminal coverage exist. |
| M6 Context autocomplete | **Not started** | No `@file` or `#symbol` picker implementation was found in the TUI. |

The roadmap also says input is locked during execution. That remains true for the TUI, but is no
longer true for the desktop client, so shared backend capability and client capability should be
reported separately.

## Provider reliability and llm-kita finding

`llm-kita` uses the OpenAI-compatible adapter. Its stream may close immediately after a valid chunk
with `finish_reason` rather than ending with the optional `[DONE]` SSE sentinel. If the gateway also
omits the final HTTP chunk terminator, reqwest/eventsource reports an EOF even though the model turn
already completed. The adapter now treats an EOF **after** a valid finish chunk as successful
termination while continuing to report errors that happen before completion. A regression test
covers this response shape.

A live llm-kita key was also embedded in the default Rust configuration. It has been removed; defaults
now read `LLMKITA_API_KEY`. Because the credential existed in Git history, it must be revoked and
replaced in the llm-kita dashboard. Existing user `config.json` files are not rewritten by this change,
so remove the old value there too and supply the replacement through configuration or the environment.
