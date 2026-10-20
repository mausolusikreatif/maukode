# Web UI Product Roadmap

This document records the product direction for the MauKode web UI (`apps/desktop`): a React
single-page app served by the daemon and used from a desktop browser or a phone. There is no native
desktop shell; the Tauri shell was removed in PR #83.

**Implementation status is tracked in [ROADMAP.md](./ROADMAP.md), not here.** This document explains
what the product should become and why, so sections below describe intent, not shipped behavior.

The comparison with other agent products reflects what was available in October 2026. Their
features will change; the durable goal is the workflow described here, not imitation.

## Product direction

MauKode should primarily be a coding-agent workspace, informed by the best general agent patterns:

- Use a **Codex-style workflow** for repositories, parallel tasks, worktrees, code review, and IDE
  integration.
- Borrow **Cowork-style concepts** for persistent projects, understandable progress, reviewable
  outputs, automations, and isolated execution.
- Preserve MauKode's differentiators: explicit permissions, private tool execution, checkpoint
  rollback, and a local or self-hosted daemon that you can reach from any device.

The target is roughly 70% coding workflow and 30% general-purpose agent workspace. Making
long-running work understandable, steerable, and safe matters more than visual similarity.

Because the UI runs in a browser, "native" integration means what the web platform offers:
notifications, installable PWA, deep links into sessions, and good phone ergonomics. Opening files
in a local editor needs a daemon endpoint rather than a desktop shell.

## Target layout

A coding-focused three-pane layout on wide screens; on phones each pane is its own screen behind
the bottom tab bar:

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

Keep the paper/ink visual language from the rebuild (Martian Mono, hairline rows, cobalt reserved
for Send).

## Priority 0: a credible coding workspace

### Project selection

Start a session in a folder quickly: recent repositories, a project's "+ New" shortcut, and a
worktree toggle. Next steps: clone from a Git URL, a server-side folder browser (the browser cannot
see the daemon's filesystem), and useful onboarding on the empty home screen.

### Composer context and attachments

`@` context chips for files, folders, symbols, and the current changes are the base. Add image
and file upload, clipboard paste, and drag-and-drop.

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

Plans, step state, and subagent status are shown in the thread. Add progress summaries,
verification lifecycle events (tests started/passed/failed), an explicit waiting-for-input state,
and a richer disclosure model for raw activity.

### Steering and queued prompts

Mid-turn steering and follow-up queueing exist. Add editable and reorderable queues and clearer
interrupt-and-redirect recovery.

### Session search and organization

Project grouping, search, pin, and rename exist. Pin state lives in browser storage, so it does not
follow you to another device; move session organization (pin, archive, tags) into the daemon store.
Add archive, full-text search, repository/branch/date filters, and unread/changed/needs-review
markers.

### Notifications

Browser notifications fire for approvals and finished turns of sessions you are not looking at.
Add click-through to the session, per-event preferences, and Web Push so a phone gets notified
while the tab is closed.

### Review workflow

The review panel shows per-file diffs against the first checkpoint, undo of the last turn, and
worktree commit and merge. The desired loop is:

```text
agent completes
    → changed files appear
    → user annotates a line
    → agent applies feedback
    → checks rerun
    → user merges or opens a pull request
```

Missing pieces: side-by-side and syntax-highlighted diffs, expandable context, line comments with
"ask agent to fix this", per-file or per-hunk revert, adjacent test results, worktree discard,
push, and pull-request creation.

### Keyboard-first navigation

Bring back a command palette and shortcut sheet (lost in the rebuild) for task/session navigation,
needs-you, review, composer, stop, and search.

## Priority 1: approach the Codex workflow

### Skills and reusable workflows

The daemon discovers repository and user-level skills (`/api/v1/skills`); Settings lists them and
toggles plugins. The direction:

- Inspect skills with defined inputs and outputs.
- Slash commands and autocomplete in the composer.
- Create a reusable skill from a successful session.
- One capability browser for MCP tools, plugins, and skills.

Potential built-ins: `/review-pr`, `/fix-ci`, `/prepare-release`, `/update-dependencies`,
`/investigate-bug`.

### Automations and scheduled tasks

Needs a backend model first (schedule entities, run history, retry policy). Then:

- Select repository, branch, environment, and permission policy.
- Show run history and outputs; notify on completion or failure.
- Pause and resume schedules.
- Suggest an automation after a repeated successful task.

Examples: reviewing dependency updates, checking failing CI, a weekly repository health report,
triaging new issues.

### Rich execution details

A dedicated inspector for the shell stream, test and diagnostics summary, context and cost,
elapsed time, files read, network requests, and the execution backend (host, bubblewrap, or lawang
cell) of each command.

### IDE and source-control integration

Open a file in a local editor through a daemon endpoint, terminal launch, GitHub/GitLab issue and
PR context, CI status, rebase assistance, push, and PR creation.

## Priority 2: Cowork-inspired capabilities

These should follow the core coding workflow rather than delay it.

### Artifacts and output viewer

Treat useful outputs as reviewable objects, not only message text: rendered HTML and Markdown,
Mermaid diagrams, screenshots, coverage and test reports, generated documents, live app previews
(the daemon's dev-server preview already exists), and downloadable bundles. The right-hand
inspector can expose `Review`, `Preview`, `Tests`, and `Terminal` tabs. Agent-generated HTML must
run in a separate, capability-restricted origin, never in the UI's own origin.

### Isolated browser and computer use

Start with a coding-focused browser environment: an isolated profile per session, screenshots and
DOM inspection, local dev-server preview, console and network logs, user takeover, domain
allowlists with per-domain approval, and credentials supplied by a broker rather than through the
model context. Never automate the user's normal browser profile.

### Persistent project objects

Grouping sessions by directory is not a project model. Introduce a project entity with a display
name and root folder; default provider, model, and permission mode; persistent instructions and
memory; enabled MCP servers and skills; environment configuration; session history; and
automation definitions.

### Remote and cross-device sessions

The daemon is already reachable from a phone (for example over Tailscale). Build on that with
secure device pairing instead of copying a URL and token, revocable device sessions, push
notifications, and clear local-versus-remote execution indicators.

## Claude Cowork interface model

The durable, task-oriented model is:

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

The relevant principles:

1. The goal is primary; chat is the control surface.
2. Progress is summarized as meaningful work.
3. Raw activity remains inspectable.
4. The user can leave and return later.
5. Outputs have a dedicated review surface.
6. File, connector, browser, and permission capabilities are explicit.
7. Projects preserve context across related tasks.

## Execution harness

MauKode's current architecture:

```text
Web UI / TUI / CLI
   │ HTTP/SSE (Bearer token, or loopback-only without one)
MauKode daemon
   │
Agent loop
   ├── permission manager and bash risk assessment
   ├── path jail for file tools
   ├── bash: host process (default), bubblewrap, or a lawang cell
   ├── MCP processes (host)
   └── git worktree and checkpoint isolation
```

The path jail validates file-tool paths; it is not an operating-system sandbox. In the default
configuration `bash` is a host process, so a command can reach other paths, credentials,
processes, and the network. The UI must say which backend a session uses and must not imply
isolation that is not configured.

lawang cells (bubblewrap, gVisor, or Firecracker) provide real process isolation for `bash`. A
Cowork-grade harness still needs:

- Network disabled by default, with domain-scoped grants.
- A secret broker that gives credentials only to approved tools.
- Folder grants represented as explicit capabilities.
- Isolation for MCP servers, not only `bash`.
- Structured audit records for filesystem, process, and network activity.
- Clear `Host`, `Sandbox`, and `Remote` labels in the UI.
- Equivalent backends on macOS and Windows.

## Issue structure

Track delivery with focused, independently implementable issues, and update
[ROADMAP.md](./ROADMAP.md) when one lands. Suggested issues:

1. Restore the command palette, archive, and open-in-editor (via a daemon endpoint).
2. Notification click-through, preferences, and Web Push.
3. Attachments, clipboard paste, and drag-and-drop.
4. Progress lifecycle events and an execution inspector.
5. Review: inline feedback, split/highlighted diff, per-file/hunk revert, worktree discard, PRs.
6. Server-side session organization (pin, archive) shared across devices.
7. Skills UI and a unified capability browser.
8. Persistent projects, scheduled automations, and network/secret controls for the harness.

Each implementation issue should include:

```markdown
## Problem

## User experience

## Proposed scope

## Protocol/backend changes

## Web UI changes

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
