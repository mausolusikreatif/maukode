# MauKode desktop

Tauri 2 shell + React/TypeScript UI for managing agents. The UI talks to the MauKode daemon over
HTTP/SSE (see [`docs/API.md`](../../docs/API.md)); it holds no agent logic.

```
apps/desktop
├── src/api/generated   TypeScript types generated from the Rust API types (do not edit)
├── src/api             typed client + fetch-based SSE follower (resume via Last-Event-ID)
├── src/state           event reducer (pure, unit tested) and the zustand store
├── src/components      sidebar, thread, approvals, composer
├── e2e                 Playwright tests against a real daemon and a scripted LLM gateway
└── src-tauri           desktop shell (starts/finds the daemon, hands the UI its token)
```

## Develop in a browser (no Tauri needed)

```sh
cargo run -p maukode-cli -- server --bind 127.0.0.1:4096 --token dev   # terminal 1
cd apps/desktop && npm install && npm run dev                           # terminal 2
# open http://127.0.0.1:1420/?url=http://127.0.0.1:4096&token=dev
```

## Tests

```sh
npm run typecheck && npm test      # unit tests (SSE parser, event reducer)
cargo build -p maukode-cli         # the e2e tests drive this binary (or set MAUKODE_BIN)
npm run test:e2e                   # builds the UI, starts a daemon + scripted gateway, drives Chromium
```

## Regenerate API types

After changing a Rust type that is part of the API:

```sh
cargo test -p maukode-protocol -p maukode-core -p maukode-server \
  --features maukode-protocol/ts,maukode-core/ts,maukode-server/ts export_bindings
```

CI fails if `src/api/generated` is stale.

## Desktop shell

`src-tauri` is excluded from the root Cargo workspace because it needs the platform WebView
(WebKitGTK on Linux). Build it with `npm run tauri dev` / `npm run tauri build` after installing
the [Tauri prerequisites](https://tauri.app/start/prerequisites/). The new-session dialog uses a
native folder picker in the desktop shell and keeps the eight most recently used repositories;
browser development retains the editable path field. A Windows `.ico` and macOS `.icns` still
need to be generated with `tauri icon`.

## Reviewing agent work

The **Changes** button opens a panel with the files the agent changed (against the checkpoint taken
before the session's first turn), a line-numbered diff per file, **Undo last turn** (restores files;
the conversation is kept), and — for sessions started with worktree isolation — commit and merge
controls. Merging uses `--no-ff` into whatever branch the project currently has checked out; a
conflict is rolled back and reported, and a project with uncommitted changes to tracked files is
refused. Deleting a worktree session offers to keep (default) or discard its branch.

## Managing agents

- **Overview** lists every agent by urgency: *needs you* (pending approvals), *stopped early*
  (output/step limit or error), *running*, *recent*. The window title shows `(N) MauKode` while
  N sessions wait for approval.
- **Notifications** (Settings → General) are sent for approvals and finished turns of sessions you
  are not currently looking at. Cancelling a turn never notifies. In the Tauri shell the webview's
  Notification API may need `tauri-plugin-notification`; not wired yet.
- **Settings**: default approval mode and agent limits (persisted by the daemon), providers
  (keys are write-only), ChatGPT sign-in, and read-only views of MCP servers and security.
- **Audit** (session header) shows approval decisions, mode changes and compactions.
