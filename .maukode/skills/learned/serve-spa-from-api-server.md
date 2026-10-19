---
name: serve-spa-from-api-server
description: "Wire a pre-built SPA frontend into an existing Rust API server (axum + tower-http) with auth-aware static file serving"
run_as_subagent: false
---
When adding web UI serving to an existing API-only Rust server: (1) Add the `fs` feature to `tower-http` in Cargo.toml. (2) Add an optional `web_dir: Option<PathBuf>` field to the server config/state, with auto-discovery of common `dist/` locations. (3) In the router, after all API routes and middleware layers, attach `ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")))` via `router.fallback_service()` — this handles SPA client-side routing. (4) Ensure static assets bypass auth middleware by placing the fallback after `route_layer(auth_middleware)` scoping. (5) In the frontend, default `getBootstrap()` to `window.location.origin` when no explicit URL is provided, so the UI auto-connects to the server that served it. (6) Support token via URL fragment (`#token=...`) for bookmarkable mobile access, stripping the fragment after reading.
