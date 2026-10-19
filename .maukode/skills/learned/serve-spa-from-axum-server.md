---
name: serve-spa-from-axum-server
description: "Add static SPA file serving to an existing axum API server using tower-http ServeDir as a fallback"
run_as_subagent: false
---
To serve a pre-built SPA from an axum API server: (1) Add the `fs` feature to `tower-http` in Cargo.toml. (2) Store an optional `web_dir: Option<PathBuf>` in your server state struct. (3) In `create_router`, after building all API routes, add `ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")))` as a `fallback_service` on the router — this serves static assets for unmatched paths and falls back to index.html for client-side routing. (4) Ensure static assets bypass auth middleware by applying auth only via `.route_layer()` on API routes, not as a global layer. (5) Auto-discover the web directory by checking common paths (`./apps/desktop/dist/`, `./dist/`, relative to binary) when not explicitly provided. (6) In the frontend, default `getBootstrap()` to `window.location.origin` when no URL is specified, so the UI auto-connects to the server that served it.
