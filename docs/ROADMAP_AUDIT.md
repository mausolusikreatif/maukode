# Roadmap Audit

Originally audited against `master` at `434e12c`; re-audited and corrected on October 7, 2026
against `0d62650` (PRs #45–#47, the OpenAI-compatible SSE reliability and `llm-kita` credential
follow-ups). No desktop or TUI product feature landed between those commits, so the capability
conclusions below stand; only the provider-reliability section and the stale-evidence claims were
updated. This is a code-evidence snapshot, not a replacement for the product direction in
`DESKTOP_ROADMAP.md` and `TUI_ROADMAP.md`.

## Executive summary

Both roadmaps are useful design documents, but neither is a reliable implementation tracker. The
desktop roadmap describes several shipped features as future work, while the TUI milestone table has
not been updated after M1, M4, and M5 landed. Keep product rationale in those documents and track
status in one checklist (this file or issues) to avoid implementing the same feature twice.

Headline findings:

- The desktop client has effectively delivered Priority 0 / Milestone 1 and most of Milestone 2, yet
  `DESKTOP_ROADMAP.md` previously presented those capabilities as future proposals. The roadmap now
  labels current work **done** / **partial** / **planned** with concrete evidence.
- The TUI lags behind the backend and desktop client. Steering/queuing and context autocomplete are
  backend/desktop features without TUI surfaces; TUI M1, M4, and M5 are done, while M2, M3, and M6
  remain unfinished client work.
- Skills and plugins have backend endpoints but no usable desktop management surface. Priority 2
  automation, browser/computer use, artifacts, persistent projects, and remote pairing have no
  implementation evidence.
- Pin/archive organization is persisted client-side in `localStorage`; project/session metadata should
  be reconciled with the server/session store before it is treated as portable cross-device state.

## Desktop roadmap

### Shipped or substantially shipped

- Project/session dashboard, grouping, title rename, persisted local pin/archive state, and status
  filtering.
- Native directory picker and recent-repository selection in `NewSessionDialog.tsx`.
- Worktree creation plus commit, merge, and discard flows.
- Resumable session SSE, permission decisions, checkpoints/undo, per-file review, steering, and
  follow-up queueing.
- Structured plan timeline, live plan-step state, subagent cards, and trace drawer.
- Command palette, keyboard shortcuts, completion notifications, and composer context autocomplete
  (`@file`, `@folder`, `#symbol`) with chips/token estimate.
- Backend skills and plugin discovery/enable/disable endpoints; no desktop management UI yet.

### Partial

- Skills exist in the backend, but the desktop capability browser, authoring flow, and composer
  slash-command integration are still missing.
- MCP is implemented and scoped, but unified MCP/skill/plugin management in the desktop is incomplete.
- Session organization uses client-side metadata in places; it is not yet a fully persisted server-side
  project model.
- Native integration has a folder picker and daemon lifecycle, but menus/tray, deep links, updater,
  release channels, editor launch actions, and polished crash recovery remain roadmap work.
- Composer context references and token estimation are implemented for files, folders, and symbols;
  image/file upload, clipboard paste, drag-and-drop, and git-diff context remain unimplemented.
- Review has per-file unified diff, refresh, editor/file-manager actions, checkpoint undo, and
  worktree commit/merge/discard; line-level review, hunk/file revert, test results, and PR workflow
  remain unimplemented.
- Notifications are browser-level completion/approval/error alerts, not native Tauri notifications
  with click routing or OS actions.

### Not evidenced in the current tree

- Scheduled/background automations with history and retry policy.
- A restricted browser/computer-use runtime.
- The stronger isolated execution architecture described in the security section.
- Desktop artifact/output preview tabs and persistent project objects.
- Remote device pairing and cross-device session continuation.

### Documentation correction needed

The desktop roadmap now labels its current capabilities and remaining work. Keep that convention for
new sections: use **done**, **partial**, **backend only**, or **planned**, and include a concrete file,
route, or protocol symbol for completed claims.

## TUI roadmap

| Milestone | Audited status | Evidence / remaining work |
|---|---|---|
| M1 Plan widget & events | **Done** | `active_plan`, `PlanCreated`, `PlanStepUpdated`, `/plan`, F2, and `draw_plan_widget` exist. The roadmap's single `PlanUpdate` event name is stale. |
| M2 Steering & queueing | **Backend only** | Server routes/state and desktop controls exist; TUI input is rejected while running and has no steering key handlers or queue badge. |
| M3 Session explorer | **Basic only** | TUI has a `/sessions` selector, but no pin/archive controls, status tabs, or full explorer organization. Desktop has these features, persisted locally. |
| M4 Subagent stream widgets | **Done** | Protocol subagent events, in-stream cards, status/tokens, and TUI rendering are present; add cancellation and stronger event-stream coverage. |
| M5 Subagent trace modal | **Done** | `ActiveModal::SubagentViewer`, trace rendering, scrolling, and virtual-terminal regression coverage exist. Concurrent switching/split-pane UX is incomplete. |
| M6 Context autocomplete | **Desktop only** | Desktop composer has `@file`, `@folder`, and `#symbol` cached-map autocomplete. No equivalent TUI popup picker or insertion flow exists. |

### TUI phase/track corrections

- Track 2.1 is implemented, but the protocol uses `PlanCreated` and `PlanStepUpdated` rather than a
  single `LLMEvent::PlanUpdate`; retain the requested UX as direction, not as an API contract.
- Track 2.2 remains unimplemented in the TUI even though the server and desktop support it.
- Track 2.3 is only a basic selector in the TUI; its advertised pin/archive/filter-chip experience
  is still missing.
- Track 2.4 is implemented in the desktop, not in `crates/maukode-tui`.
- Phase 3's cards and trace inspector are present. Number-key concurrent switching and the wide
  split-pane layout described in sections 4.3–4.4 are still planned.

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
