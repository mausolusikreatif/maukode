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
the [Tauri prerequisites](https://tauri.app/start/prerequisites/). It was written against the
Tauri 2 API but has **not been compiled yet** (no WebView libraries in the authoring
environment); a Windows `.ico` and macOS `.icns` still need to be generated with `tauri icon`.
