pub mod client;
pub mod daemon;
pub mod middleware;
pub mod routes;
pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use tokio::net::TcpListener;
use tracing::info;
pub use client::ServerClient;
pub use daemon::{ensure_daemon_running, get_daemon_status, stop_daemon, DaemonInfo};
pub use state::ServerState;

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
    let state = Arc::new(ServerState::new(config.auth_token.clone(), config.default_dir).await?);
    let app = routes::create_router(state);

    let listener = TcpListener::bind(config.bind_addr).await?;
    info!("🚀 MauKode Server listening on http://{}", listener.local_addr()?);
    if config.auth_token.is_some() {
        info!("🔒 Bearer authentication enabled");
    }

    axum::serve(listener, app).await?;
    Ok(())
}
