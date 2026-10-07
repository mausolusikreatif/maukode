# Desktop Product Roadmap

> Implementation status was last audited on October 7, 2026 in
> [ROADMAP_AUDIT.md](./ROADMAP_AUDIT.md). This document contains product direction; unmarked
> proposals below are not necessarily unimplemented.

This document records the product direction for the MauKode desktop application. It compares the
current application with agent-oriented desktop products such as the Codex app and Claude Cowork,
then turns the gaps into an implementation sequence.

The comparison reflects the products and documentation available on October 6, 2026. Competitor
features will change; the durable goal is the workflow described here, not pixel-for-pixel imitation.

## Product direction

MauKode should primarily be a coding-agent workspace, informed by the best general agent patterns:

- Use a **Codex-style workflow** for repositories, parallel tasks, worktrees, code review, and IDE
  integration.
- Borrow **Cowork-style concepts** for persistent projects, understandable progress, reviewable
  outputs, automations, and isolated execution.
- Preserve MauKode's explicit permissions, private tool execution, checkpoint rollback, and local or
  self-hosted daemon model as differentiators.

The target is approximately 70% coding workflow and 30% general-purpose agent workspace. Visual
similarity is less important than making long-running work understandable, steerable, and safe.

## Current foundation

The desktop application already supports:

- Multiple concurrent agent sessions grouped by project, with sidebar search, status filters, rename,
  pin, archive/restore, and persisted organization.
- A dashboard organized by approvals, interrupted tasks, running tasks, and recent tasks.
- Git worktree isolation with commit, merge, and discard flows.
- Streaming messages, reasoning, tool calls, and tool results over resumable SSE.
- Permission decisions to allow once, allow for a session, run privately, or deny.
- Per-file change review with unified diffs, plus editor and file-manager reveal actions.
- Turn checkpoints and undo.
- Provider and model selection.
- MCP and security status views.
- Session audit history.
- Structured plan timelines, subagent cards, and a subagent trace drawer.
- Composer `@file`/`@folder`/`#symbol` context chips with a token estimate, plus mid-turn steering and
  prompt queuing.
- Command palette and keyboard shortcut reference.
- Light and dark themes.
- A local Tauri shell that starts or discovers the MauKode daemon, a native folder picker, recent
  repositories, and browser-level completion/approval/error notifications.

This is a strong agent and review foundation. Remaining gaps are the unfinished portions of the
sections below, the Priority 1 surfaces (skills/plugin management, richer execution details, IDE and
source-control integration), and all Priority 2 capabilities. See
[ROADMAP_AUDIT.md](./ROADMAP_AUDIT.md) for feature-by-feature code evidence.

## Target desktop layout

A coding-focused three-pane layout should build on the current sidebar, thread, and review panel:

```text
┌─────────────────────┬────────────────────────────────┬──────────────────────┐
│ Search              │ project / branch       ⋯       │ Changes          ×   │
│ + New task          │                                │                      │
│                     │ Goal                           │ src/auth.rs      +8-2 │
│ Needs you       2   │                                │ src/auth_test.rs +31  │
│ Running         3   │ Plan                           │                      │
│                     │ ✓ Inspect implementation       │ [Unified|Split]      │
│ my-project          │ ◉ Implement fix                │                      │
│   Fix auth race  ●  │ ○ Run tests                    │ selected diff        │
│   Upgrade deps      │                                │                      │
│ other-project       │ compact activity timeline      │ [Comment on line]    │
│   Add export        │                                │                      │
│                     │ final answer / questions       │ Tests: 42 passed     │
│ Automations         ├────────────────────────────────┤                      │
│ Settings            │ @file  Attach                  │ [Ask changes]        │
│                     │ Message…        Queue ▾   ↑    │ [Merge]              │
└─────────────────────┴────────────────────────────────┴──────────────────────┘
```

Keep the existing graphite visual language. Improve hierarchy, spacing, status visibility, composer
affordances, empty states, and native window integration instead of doing a wholesale visual rewrite.

## Priority 0: credible coding desktop experience

### Native project selection — **done / partial**

The new-session dialog now has a native Tauri folder picker, recent repositories, and repository
remembering (`NewSessionDialog.tsx`, `platform.ts`, and `state/repositories.ts`). Remaining work is:

- Drag-and-drop folders onto the window.
- Clone from Git URL.
- An operating-system "Open in MauKode" integration.
- Recent folders and useful onboarding on the empty home screen.

### Composer context and attachments — **partial**

The composer now supports `@file`, `@folder`, and `#symbol` suggestions, context chips, a context
token estimate, attachment-menu affordances, and model, permission, and worktree controls. Remaining
work includes image/file upload, clipboard image paste, drag-and-drop, and `@git-diff` support.

Example:

```text
┌───────────────────────────────────────────────────────────────┐
│ @src/auth.rs  @current-diff                                   │
│                                                               │
│ Fix the token refresh race and add regression tests…          │
│                                                               │
│ ＋ Attach    Local ▾    Ask before risky actions ▾    Send ↑  │
└───────────────────────────────────────────────────────────────┘
```

### Plans, progress, and activity — **partial**

Structured plan creation/update events, live step state, usage, and subagent status are implemented
in the protocol and desktop (`PlanTimeline.tsx`, `Thread.tsx`). Progress summaries, verification
lifecycle events, explicit waiting-for-input state, and a richer activity disclosure model remain
planned.

### Steering and queued prompts — **done (desktop) / partial (TUI)**

The server exposes steering and follow-up queueing, and the desktop composer provides both controls
(`store.ts`, `Composer.tsx`). The TUI still rejects input while a turn is running and has no queue
badge or steering key handlers; track those separately in `TUI_ROADMAP.md`. Editable/reorderable
queue semantics and clearer interrupt-and-redirect recovery remain planned.

### Session search and organization — **partial**

The desktop has project grouping, status filters, title rename, pin, archive/restore, and persisted
local session organization (`Sidebar.tsx`, `state/sessionFilters.ts`, `state/store.ts`). Full-text
search, repository/branch/date filters, project instructions/memory, and stronger unread/changed/
needs-review indicators remain planned.

### Native notifications — **partial**

The client emits browser completion/approval/error notifications when the window is hidden or
unfocused and respects notification permission (`state/notify.ts`, `platform.ts`). Native Tauri
notifications, click-through session routing, badges, sounds, preferences, and action buttons remain
planned.

### Review workflow — **partial**

The current panel provides a per-file unified diff, file selection, refresh, editor and file-manager
open actions, checkpoint undo, and worktree commit, merge, and discard (`ReviewPanel.tsx`,
`state/store.ts`).

Side-by-side rendering and syntax highlighting, expandable hidden context, line comments and "Ask
agent to fix this", per-file or per-hunk accept/revert, adjacent test results, pull-request
creation, and explicit review states remain planned.

The desired loop is:

```text
agent completes
    → changed files appear
    → user annotates a line
    → agent applies feedback
    → checks rerun
    → user merges or opens a pull request
```

### Keyboard-first navigation — **done (web/desktop client) / partial (native shell)**

`CommandPalette.tsx` and `ShortcutModal.tsx` cover task/session navigation, needs-you, review,
sidebar/inspector, composer, stop, search, and shortcut help. Application-level native menus remain
planned.

### Native application polish — **partial**

The Tauri shell starts and supervises the daemon and provides folder selection, editor, and file
manager integration. Menus, a system tray, taskbar badges, deep links, single-instance behavior,
auto-updates/release channels, crash recovery, remembered window layout, and polished lifecycle/status
surfaces remain planned.

## Priority 1: approach the Codex workflow

### Skills and reusable workflows — **backend only**

Repository and user-level skills are discovered and expanded by the daemon (`/api/v1/skills`,
`SkillManager`), but the desktop has no skills browser, no enable/disable or inspection UI, no
reusable-session authoring flow, and no composer slash-command integration. The full proposal is:

- Browse, enable, disable, and inspect repository and user-level skills.
- Defined inputs and outputs.
- Slash-command and composer autocomplete.
- Create a reusable skill from a successful session.
- Present MCP tools, plugins, and skills in one capability browser.

Potential built-ins include `/review-pr`, `/fix-ci`, `/prepare-release`,
`/update-dependencies`, and `/investigate-bug`.

### Automations and scheduled tasks — **planned**

There are currently no daemon schedule entities, run history, retry policy, desktop automation
controls, or related protocol types in this repository. This section remains design direction until
the backend model exists.

- Select repository, branch, environment, and permission policy.
- Show run history and artifacts.
- Notify on completion or failure.
- Pause and resume schedules.
- Suggest an automation after a repeated successful task.

Examples include reviewing dependency updates, checking failing CI, producing a weekly repository
health report, and triaging new issues.

### Rich execution details — **partial**

`Thread.tsx` already exposes usage, reasoning, tool calls, tool results, subagent status, plan state,
and permission activity. A dedicated right-hand inspector for shell stream, test/diagnostics summary,
context/cost, elapsed time, files read, network requests, and redacted environment details is not
implemented.

### IDE and source-control integration — **partial**

The desktop can open a file in VS Code or Cursor and reveal it in the file manager; local worktree
commit/merge/discard is implemented (`platform.ts`, `App.tsx`, `ReviewPanel.tsx`). Terminal launch,
a local IDE bridge, GitHub/GitLab issue/PR context, CI status, rebase assistance, push, and PR
creation remain planned.

## Priority 2: Cowork-inspired capabilities — **planned**

These should follow the core coding workflow rather than delaying it. None of the capabilities below
has desktop, daemon, or protocol implementation evidence.

### Artifacts and output viewer

Treat useful outputs as reviewable objects, not only message attachments:

- Rendered HTML and Markdown.
- Mermaid diagrams.
- Screenshots.
- Coverage and test reports.
- Generated documents and PDFs.
- Interactive application previews.
- Downloadable output bundles.

The right-hand inspector can expose `Review`, `Preview`, `Tests`, and `Terminal` tabs. Arbitrary
agent-generated HTML must run in a separate, capability-restricted origin rather than the primary
application webview.

### Isolated browser and computer use

Begin with a coding-focused browser environment:

- An isolated browser profile per session.
- Screenshots and DOM inspection.
- Local development-server preview.
- Console and network logs.
- User takeover.
- Domain allowlists and per-domain approval.
- Credentials supplied through a broker rather than model context.

Do not run unrestricted automation against the user's normal browser profile.

### Persistent project objects

Grouping sessions by `project_dir` is useful but is not a persistent project model. Introduce a
project entity with:

- Display name and root folder.
- Default provider, model, and permission mode.
- Persistent instructions and memory.
- Enabled MCP servers and skills.
- Environment configuration.
- Session history.
- Automation definitions.

### Remote and cross-device sessions

Build on the daemon architecture with:

- Secure device pairing instead of manually copying URL and token.
- A remote session list and push notifications.
- Continue-from-another-device support.
- Explicit local versus remote execution indicators.
- Revocable device sessions.
- Intentional upload or synchronization of remote workspace context.

## Claude Cowork interface model

Cowork has evolved into the unified Claude experience, but its durable task-oriented model is:

```text
┌────────────────┬──────────────────────────────────┬────────────────────┐
│ Projects       │ Task / conversation              │ Output / preview   │
│                │                                  │                    │
│ Project A      │ User goal                        │ Document           │
│  Task 1        │                                  │ Spreadsheet        │
│  Task 2        │ Plan / progress                  │ Browser            │
│                │  ✓ Gather inputs                 │ Artifact           │
│ Project B      │  ◉ Analyze files                 │                    │
│                │  ○ Produce result                │                    │
│ Scheduled      │                                  │                    │
│ tasks          │ Activity, questions, approvals,  │                    │
│                │ and final result                 │                    │
│ Artifacts      │                                  │                    │
│                ├──────────────────────────────────┤                    │
│                │ Attach / tools / task composer   │                    │
└────────────────┴──────────────────────────────────┴────────────────────┘
```

The relevant principles are:

1. The goal is primary; chat is the control surface.
2. Progress is summarized as meaningful work.
3. Raw activity remains inspectable.
4. The user can leave and return later.
5. Outputs have a dedicated review surface.
6. File, connector, browser, and permission capabilities are explicit.
7. Projects preserve context across related tasks.

MauKode already has the basic left and center areas. Its review panel should evolve into the
right-hand output inspector.

## Cowork-style execution harness

Anthropic's published Cowork architecture describes separate local and remote execution modes,
with task capabilities mediated instead of giving the model an unrestricted desktop process. The
conceptual structure is:

```text
Desktop, web, or mobile UI
        │
        ├── task state, approvals, progress, and artifacts
        │
Agent orchestration
        │
        ├── model calls
        ├── tool policy
        ├── connector credential broker
        └── audit and telemetry
        │
Isolated execution environment
        ├── temporary filesystem
        ├── shell and task tools
        ├── scoped mounted folders
        ├── restricted network path
        └── browser or computer-use bridge
```

MauKode currently has a different and pragmatic local architecture:

```text
Tauri UI
   │ HTTP/SSE with bearer token
Local MauKode daemon
   │
Agent loop
   ├── path-jail checks
   ├── permission manager
   ├── direct host shell process
   ├── file tools
   ├── MCP processes
   └── git worktree and checkpoint isolation
```

The path jail is a useful path-validation layer, but it is not an operating-system sandbox. The bash
tool launches a host shell; constraining its working directory does not stop a command from reaching
other host paths, credentials, processes, or the network. Product language and UI should not imply
strong process isolation until an OS-level boundary exists.

A Cowork-grade MauKode harness should eventually provide:

- Linux namespaces, seccomp, bubblewrap, containers, or a lightweight VM.
- A sandboxed helper or VM-backed runner on macOS.
- AppContainer or Hyper-V-backed execution on Windows.
- Network disabled by default with domain-scoped grants.
- A secret broker that gives credentials only to approved tools.
- Folder grants represented as explicit capabilities.
- A separate workspace per task or worktree.
- Structured audit records for filesystem, process, and network activity.
- Clear `Host`, `Sandbox`, and `Remote` execution labels.

## Delivery milestones

Current status of each milestone is labelled from the code audit; details in
[ROADMAP_AUDIT.md](./ROADMAP_AUDIT.md).

### Milestone 1: desktop usability — **substantially complete**

1. Native folder picker and recent repositories — **done**; drag-and-drop, Git URL clone, and OS
   "open in MauKode" integration remain.
2. Session rename, search, archive, and pin — **done** in the desktop client (title search, status
   filters, rename, pin, archive/restore); server-portable full-text search remains.
3. Native notifications — **partial**; alerts currently use the browser Notification API, with no
   click-through routing yet.
4. Composer model, mode, and worktree controls — **done**.
5. Keyboard shortcuts and command palette — **done** in the app; native application menus remain.
6. Open or reveal in editor and file manager — **done** (VS Code/Cursor and file-manager reveal);
   terminal launch and more IDE targets remain.

### Milestone 2: agent steering — **partial**

1. Structured plan and progress protocol events — **done** for plans, steps, subagent status, and
   usage; progress summaries and verification lifecycle events remain.
2. Queued and steering messages — **done** in the server and desktop composer; **planned** in the TUI.
3. Explicit clarification requests — **partial**; approvals/interrupts are surfaced, a dedicated
   clarification event is not.
4. Better dashboard states and unread indicators — **partial**; needs-you/running/interrupted/recent
   exist, unread and changed markers do not.
5. Usage, duration, and context display — **partial**; usage and context estimate exist, a richer
   activity inspector remains.

### Milestone 3: review product — **partial**

1. Syntax-highlighted split diff — planned (unified per-file diff is shipped).
2. Per-file and per-hunk revert — planned; whole-turn checkpoint undo is shipped.
3. Inline feedback to the agent — planned.
4. Test-result panel — planned.
5. Commit, push, and pull-request flow — **partial**; worktree commit/merge/discard is shipped, push
   and PR creation are not.

### Milestone 4: reusable autonomous work — **planned**

Persistent project entity, skills UI, automations/scheduling, automation history, and remote pairing
have no desktop implementation; skills/plugin endpoints exist backend-side only.

### Milestone 5: hardened agent harness — **planned**

OS-level sandbox runner, network capability controls, secret broker, isolated browser, artifact and
preview viewer, and optional remote execution remain design work.

## Issue structure

Keep this document as the product reference. Track delivery with one roadmap issue and focused,
independently implementable issues. Replace stale "add" wording with current gaps:

1. Add drag/drop, Git URL clone, and OS open-in-MauKode integration.
2. Wire native notifications, click-through session deep links, menus/tray, and lifecycle recovery.
3. Add attachment upload, clipboard/drag-drop context, and `@git-diff`.
4. Add rich progress lifecycle events and an execution inspector.
5. Add TUI steering/queueing, session organization, and context autocomplete parity.
6. Add desktop skills/plugin management and unified capability browsing.
7. Improve diff review with inline feedback, syntax-highlighted split view, and per-file/hunk revert.
8. Design persistent projects, scheduled automations, and an OS-isolated execution harness.

Each implementation issue should include:

```markdown
## Problem

## User experience

## Proposed scope

## Protocol/backend changes

## Desktop changes

## Security considerations

## Acceptance criteria

## Out of scope
```

## References

- [OpenAI: Introducing the Codex app](https://openai.com/index/introducing-the-codex-app/)
- [OpenAI desktop app documentation](https://developers.openai.com/codex/app.md)
- [Claude Cowork product page](https://claude.com/product/cowork)
- [Get started with Claude Cowork](https://support.claude.com/en/articles/13345190-getting-started-with-cowork)
- [Claude Cowork architecture overview](https://support.claude.com/en/articles/14479288-claude-cowork-architecture-overview)
- [Projects in Claude Cowork](https://support.claude.com/en/articles/14116274-organize-your-tasks-with-projects-in-cowork)
