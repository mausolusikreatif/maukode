pub mod activity;
pub mod client;
pub mod daemon;
pub mod middleware;
pub mod routes;
pub mod state;

use anyhow::Result;
pub use client::ServerClient;
pub use daemon::{ensure_daemon_running, get_daemon_status, stop_daemon, DaemonInfo};
pub use state::ServerState;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub auth_token: Option<String>,
    pub default_dir: Option<PathBuf>,
    /// Optional directory containing the pre-built web UI (e.g. Vite `dist/`).
    /// When set, the server serves these files as a SPA fallback so the UI is
    /// accessible directly from the server URL in any browser — no desktop app
    /// or dev server required.
    pub web_dir: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:4096".parse().unwrap(),
            auth_token: None,
            default_dir: None,
            web_dir: None,
        }
    }
}

/// Refuse to expose the agent beyond this machine without authentication. A token-less server
/// relies on the loopback-only Host and Origin checks, which `run_server` turns off for other
/// addresses, so anyone who can reach the port could run commands as this user.
fn check_bind_auth(bind_addr: SocketAddr, token: Option<&str>) -> Result<()> {
    if !bind_addr.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "Refusing to listen on {bind_addr} without authentication: anyone who can reach this \
             address could run commands on this machine. Pass --token <secret> (or set \
             MAUKODE_AUTH_TOKEN), or bind to 127.0.0.1."
        );
    }
    Ok(())
}

/// Auto-discover the pre-built web UI directory (`apps/desktop/dist/`) relative to common starting
/// points: the current working directory, the server binary's location, and the default workspace
/// directory. Returns `None` if no `index.html` is found.
fn discover_web_dir() -> Option<PathBuf> {
    /// Check if `dir` looks like a web build (has `index.html`).
    fn is_web_dir(dir: &Path) -> bool {
        dir.join("index.html").is_file()
    }

    // 1. Current working directory
    if let Ok(cwd) = std::env::current_dir() {
        for candidate in [
            cwd.join("apps").join("desktop").join("dist"),
            cwd.join("dist"),
        ] {
            if is_web_dir(&candidate) {
                return Some(candidate);
            }
        }
    }

    // 2. Directory containing the running server binary (release layout)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            for candidate in [
                bin_dir.join("web"),
                bin_dir.join("..").join("web"),
                bin_dir.join("apps").join("desktop").join("dist"),
            ] {
                if is_web_dir(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

pub async fn run_server(config: ServerConfig) -> Result<()> {
    // An empty token would accept `Bearer ` and switch off the local-only checks.
    let auth_token = config.auth_token.filter(|t| !t.trim().is_empty());
    check_bind_auth(config.bind_addr, auth_token.as_deref())?;
    let mut state = ServerState::new(auth_token.clone(), config.default_dir).await?;
    if !config.bind_addr.ip().is_loopback() {
        state.enforce_local_host = false;
    }
    state.web_dir = config.web_dir.or_else(discover_web_dir);
    if let Some(ref web_dir) = state.web_dir {
        info!(?web_dir, "Web UI enabled — serving static files");
    }
    // Reap sessions that were created but never received a message: a client opening a session
    // persists an empty row immediately, and if it never talks those rows accumulate forever.
    // Run once on real daemon startup — not in `ServerState::new` — so tests that construct
    // several states against one shared database don't sweep each other's fresh sessions.
    match state.store.delete_empty_sessions().await {
        Ok(removed) if removed > 0 => {
            info!(removed, "Cleaned up empty sessions on startup");
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "Failed to clean up empty sessions on startup"),
    }
    let state = Arc::new(state);
    let app = routes::create_router(state);

    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(
        "🚀 MauKode Server listening on http://{}",
        listener.local_addr()?
    );
    if auth_token.is_some() {
        info!("🔒 Bearer authentication enabled");
    }

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_bind_auth;
    use std::net::SocketAddr;

    #[test]
    fn non_loopback_bind_requires_a_token() {
        let addr = |s: &str| s.parse::<SocketAddr>().unwrap();
        assert!(check_bind_auth(addr("0.0.0.0:4096"), None).is_err());
        assert!(check_bind_auth(addr("192.168.1.5:4096"), None).is_err());
        assert!(check_bind_auth(addr("[::]:4096"), None).is_err());
        assert!(check_bind_auth(addr("0.0.0.0:4096"), Some("secret")).is_ok());
        assert!(check_bind_auth(addr("127.0.0.1:4096"), None).is_ok());
        assert!(check_bind_auth(addr("[::1]:4096"), None).is_ok());
    }
}
