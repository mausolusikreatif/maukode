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
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub auth_token: Option<String>,
    pub default_dir: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:4096".parse().unwrap(),
            auth_token: None,
            default_dir: None,
        }
    }
}

pub async fn run_server(config: ServerConfig) -> Result<()> {
    let mut state = ServerState::new(config.auth_token.clone(), config.default_dir).await?;
    if !config.bind_addr.ip().is_loopback() {
        state.enforce_local_host = false;
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
    if config.auth_token.is_some() {
        info!("🔒 Bearer authentication enabled");
    }

    axum::serve(listener, app).await?;
    Ok(())
}
