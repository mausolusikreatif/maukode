# Desktop Product Roadmap

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

- Multiple concurrent agent sessions grouped by project.
- A dashboard organized by approvals, interrupted tasks, running tasks, and recent tasks.
- Git worktree isolation and branch merge/discard flows.
- Streaming messages, reasoning, tool calls, and tool results over resumable SSE.
- Permission decisions to allow once, allow for a session, run privately, or deny.
- Per-file change review with unified diffs.
- Turn checkpoints and undo.
- Provider and model selection.
- MCP and security status views.
- Session audit history.
- Light and dark themes.
- A local Tauri shell that starts or discovers the MauKode daemon.

This is a strong agent and review foundation. The largest gaps are product-level interactions around
context, progress, steering, organization, native integration, and execution isolation.

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

### Native project selection

The new-session dialog currently requires a manually entered directory path. Add:

- A native Tauri folder picker.
- Recent repositories with repository validation.
- Drag-and-drop folders onto the window.
- Clone from Git URL.
- An operating-system "Open in MauKode" integration.
- Recent folders and useful onboarding on the empty home screen.

### Composer context and attachments

Turn the text-only composer into an explicit context surface:

- `@file`, `@folder`, `@symbol`, and `@git-diff` mentions.
- File and image attachments.
- Clipboard image paste and drag-and-drop.
- Context chips above the input.
- Context/token estimate.
- Model, permission mode, and execution environment in the composer footer.
- A visible directory and worktree capability indicator.

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

### Plans, progress, and activity

Raw tool calls are useful for inspection, but they should not be the primary progress model. Add
first-class protocol events for:

- Plan creation and updates.
- Step started, completed, or failed.
- Progress summaries.
- Verification started and completed.
- Subtask or subagent status.
- Waiting for user input.
- Final outcome.

Render a compact timeline while keeping raw activity under disclosure controls:

```text
● Inspect authentication flow
● Reproduce refresh-token race
◉ Implement synchronization             Working
○ Add regression tests
○ Run test suite
```

### Steering and queued prompts

The composer currently replaces Send with Stop while a turn is running. Long-running work needs:

- **Steer now**, injecting guidance at the next safe boundary.
- **Queue next**, starting a saved follow-up after the current turn.
- Editable and reorderable queued prompts.
- Clear interrupt, interrupt-and-redirect, and cancel-queue actions.
- Preservation of text typed while an agent is working.

### Session search and organization

Extend project grouping with:

- Full-text session search.
- Filters by repository, branch, state, and date.
- Rename, pin, archive, and restore.
- Saved project instructions and project-level notes or memory.
- Stronger unread, changed, and needs-review indicators.

### Native notifications

Replace the browser notification fallback with a native platform abstraction:

- Wire `tauri-plugin-notification`.
- Open the correct session when a notification is clicked.
- Add dock/taskbar badges and optional sound.
- Provide per-event preferences.
- Notify for approvals, clarification, completion, failure, and merge conflicts.
- Add Approve, Deny, or Open Review actions where the operating system supports them.

### Review workflow

Build on the existing diff panel:

- Unified and side-by-side modes with syntax highlighting.
- Expandable hidden context.
- Inline comments and "Ask agent to fix this" for selected lines or files.
- Accept or revert individual files and hunks.
- Open in an editor and reveal in the file manager.
- Test results adjacent to changed files.
- Commit-message editing and pull-request creation.
- Explicit unreviewed, accepted, and needs-changes states.

The desired loop is:

```text
agent completes
    → changed files appear
    → user annotates a line
    → agent applies feedback
    → checks rerun
    → user merges or opens a pull request
```

### Keyboard-first navigation

Add a command palette and shortcuts for:

- New task.
- Next or previous session.
- Jump to Needs You.
- Open review.
- Toggle sidebar or inspector.
- Focus composer.
- Stop task.
- Search sessions.
- Display the shortcut reference.

Expose appropriate application menus on macOS, Windows, and Linux.

### Native application polish

The current shell starts the daemon but otherwise has minimal native integration. Add:

- Native menus and a system tray.
- Dock/taskbar badges.
- Session deep links.
- Single-instance behavior.
- Automatic updates and release channels.
- Crash reporting and recoverable startup errors.
- Remembered window size and panel layout.
- Open/reveal actions for editors, terminals, and file managers.
- Platform-appropriate title-bar behavior and production icon assets.
- Clear daemon lifecycle and connection status.

## Priority 1: approach the Codex workflow

### Skills and reusable workflows

Introduce a first-class skills system:

- Repository skills stored in version control.
- User-level skills shared across projects.
- Browse, enable, disable, and inspect skills.
- Slash-command and composer autocomplete.
- Defined inputs and outputs.
- Create a reusable skill from a successful session.
- Present MCP tools and skills in one capability browser.

Potential built-ins include `/review-pr`, `/fix-ci`, `/prepare-release`,
`/update-dependencies`, and `/investigate-bug`.

### Automations and scheduled tasks

Add daemon-managed one-shot and recurring work:

- Select repository, branch, environment, and permission policy.
- Show run history and artifacts.
- Notify on completion or failure.
- Pause and resume schedules.
- Suggest an automation after a repeated successful task.

Examples include reviewing dependency updates, checking failing CI, producing a weekly repository
health report, and triaging new issues.

### Rich execution details

Provide an inspector with:

- Shell or terminal stream.
- Test and diagnostics summary.
- Token usage, context utilization, and estimated cost where available.
- Elapsed time.
- Files read and changed.
- Network requests.
- MCP calls.
- Agent environment with secrets redacted.

`LLMEvent::Usage` already exists, so usage can be exposed before inventing a new protocol event.

### IDE and source-control integration

Add:

- Open in VS Code, Zed, JetBrains IDEs, or a terminal.
- A local IDE bridge or extension.
- GitHub and GitLab issue and pull-request context.
- CI status and pull-request review comments.
- Branch switching and rebase assistance.
- Commit, push, and pull-request controls.

## Priority 2: Cowork-inspired capabilities

These should follow the core coding workflow rather than delaying it.

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

### Milestone 1: desktop usability

1. Native folder picker and recent repositories.
2. Session rename, search, archive, and pin.
3. Native notifications.
4. Composer model, mode, and worktree controls.
5. Keyboard shortcuts and command palette.
6. Open or reveal in editor and file manager.

### Milestone 2: agent steering

1. Structured plan and progress protocol events.
2. Queued and steering messages.
3. Explicit clarification requests.
4. Better dashboard states and unread indicators.
5. Usage, duration, and context display.

### Milestone 3: review product

1. Syntax-highlighted split diff.
2. Per-file and per-hunk revert.
3. Inline feedback to the agent.
4. Test-result panel.
5. Commit, push, and pull-request flow.

### Milestone 4: reusable autonomous work

1. Persistent project entity and instructions.
2. Skills.
3. Automations and scheduling.
4. Automation history and notifications.
5. Remote pairing.

### Milestone 5: hardened agent harness

1. OS-level sandbox runner.
2. Network capability controls.
3. Secret broker.
4. Isolated browser.
5. Artifact and preview viewer.
6. Optional remote execution.

## Issue structure

Keep this document as the product reference. Track delivery with one roadmap issue and focused,
independently implementable issues. Initial issue candidates are:

1. Add a native folder picker and recent repositories.
2. Add session search, rename, pin, and archive.
3. Wire native desktop notifications and session deep links.
4. Add composer attachments and `@file` context.
5. Add structured plans and task-progress events.
6. Allow steering and queued prompts while an agent runs.
7. Improve diff review with inline feedback and per-file revert.
8. Design an OS-isolated execution harness.

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
