---
name: serve-spa-from-axum
description: "Serve a Vite/React SPA from an Axum API server using tower-http's ServeDir with index.html fallback for client-side routing."
run_as_subagent: false
---
To serve a pre-built SPA from an Axum server: add the `fs` feature to `tower-http` in Cargo.toml, use `tower_http::services::{ServeDir, ServeFile}` as a `fallback_service` on the router so unmatched paths serve static assets with `index.html` as the SPA fallback (`ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")))`). Keep static assets outside auth middleware so the UI loads before the user enters a token. Store the `web_dir: Option<PathBuf>` on `ServerState` rather than changing `create_router`'s signature to avoid breaking test call sites. For mobile/Tailscale access, pass auth tokens via URL fragment (`#token=...`) which never reaches the server, and have the frontend default to `window.location.origin` as the API URL when loaded in a browser.
