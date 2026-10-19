---
name: serve-spa-from-axum
description: "Add static SPA file serving to an existing axum API server with SPA fallback and auto-discovery"
run_as_subagent: false
---
To serve a pre-built frontend (e.g. Vite `dist/`) from an axum API server: 1) Enable the "fs" feature on `tower-http` in Cargo.toml. 2) Store an optional `web_dir: Option<PathBuf>` in `ServerConfig` and `ServerState`.
