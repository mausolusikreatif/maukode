# MauKode web UI

React + TypeScript single-page UI served as static files by `maukode server --web-dir`. It talks to
the daemon over HTTP/SSE (see [`docs/API.md`](../../docs/API.md)) and holds no agent logic. There is
no desktop shell.

```
apps/desktop
├── public/fonts        Martian Mono (variable weight and width), self-hosted
├── src/api/generated   TypeScript types generated from the Rust API types (do not edit)
├── src/api             typed client + fetch-based SSE follower (resume via Last-Event-ID)
├── src/state           event reducer (pure, unit tested) and the zustand store
└── src/ui              sessions, thread, permission dock, composer, review panel
```

## Develop in a browser

```sh
cargo run -p maukode-cli -- server --bind 127.0.0.1:4096 --token dev   # terminal 1
cd apps/desktop && npm install && npm run dev                           # terminal 2
# open http://127.0.0.1:1420/?url=http://127.0.0.1:4096&token=dev
```

## Tests

```sh
npm run typecheck && npm test      # unit tests (SSE parser, event reducer)
```

## Regenerate API types

After changing a Rust type that is part of the API:

```sh
cargo test -p maukode-protocol -p maukode-core -p maukode-server \
  --features maukode-protocol/ts,maukode-core/ts,maukode-server/ts export_bindings
```

CI fails if `src/api/generated` is stale.

## Reviewing agent work

The **Perubahan** button opens a panel with the files the agent changed (against the checkpoint taken
before the session's first turn), a line-numbered diff per file, **Batalkan giliran terakhir** (restores files;
the conversation is kept), and — for sessions started with worktree isolation — commit and merge
controls. Merging uses `--no-ff` into whatever branch the project currently has checked out; a
conflict is rolled back and reported, and a project with uncommitted changes to tracked files is
refused. Deleting a worktree session offers to keep (default) or discard its branch.

## Managing agents

- **Overview** lists every agent by urgency: *needs you* (pending approvals), *stopped early*
  (output/step limit or error), *running*, *recent*. The window title shows `(N) MauKode` while
  N sessions wait for approval.
- **Notifications** (Settings → General) are sent for approvals and finished turns of sessions you
  are not currently looking at. Cancelling a turn never notifies. Browsers only allow notifications
  on a secure origin (`https://` or `localhost`/`127.0.0.1`).
- **Settings** (persisted by the daemon): approval mode, sandbox mode and execution backend
  (host or lawang cell), agent limits, reflection and memory consolidation, providers (keys are
  write-only), ChatGPT sign-in, plugin toggles, read-only lists of MCP servers and skills, and the
  theme.
- **Audit** (session header) shows approval decisions, mode changes and compactions.
