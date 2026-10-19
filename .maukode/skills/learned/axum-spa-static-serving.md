---
name: axum-spa-static-serving
description: "Serve a pre-built SPA (e.g., Vite dist) as a static fallback from an axum API server, with SPA index.html routing."
run_as_subagent: false
---
To serve a frontend SPA from an axum API server: add the `fs` feature to `tower-http`, use `ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")))` as a `fallback_service` on the router. Static assets bypass auth middleware (they're public UI), while API routes keep their auth layer. Store the web directory path in shared server state rather than changing the router function signature. For auto-discovery, check candidate paths relative to CWD (e.g., `./apps/desktop/dist/`, `./dist/`) when no explicit path is provided. The SPA fallback ensures client-side routing works for all non-API paths.
