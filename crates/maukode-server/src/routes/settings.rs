//! Read-mostly views of server configuration for a settings screen.
//!
//! Deliberately limited: MCP servers and the bash whitelist execute or authorise commands, so they
//! are shown but not editable over HTTP (edit the config file). The default approval mode, agent
//! limits and providers are editable.

use crate::state::ServerState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use maukode_core::PermissionMode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SecuritySettings {
    /// Approval mode new sessions start with.
    pub permission_mode: PermissionMode,
    pub jail_path_traversal: bool,
    pub allowed_external_paths: Vec<String>,
    /// Shown for information; changing it requires editing the config file and restarting.
    pub bash_whitelist: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetDefaultPermissionModeRequest {
    pub permission_mode: PermissionMode,
}

pub async fn get_security(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let config = state.config.read().await;
    Json(SecuritySettings {
        permission_mode: state.permission_manager.mode(),
        jail_path_traversal: config.security.jail_path_traversal,
        allowed_external_paths: config
            .security
            .allowed_external_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        bash_whitelist: config.security.bash_whitelist.clone(),
    })
}

/// Persist the default approval mode and apply it to sessions created from now on.
pub async fn set_default_permission_mode(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SetDefaultPermissionModeRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut config = state.config.write().await;
    config.security.permission_mode = req.permission_mode;
    config
        .save()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.permission_manager.set_mode(req.permission_mode);
    Ok(Json(
        serde_json::json!({ "status": "ok", "permission_mode": req.permission_mode }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct McpQuery {
    pub directory: Option<String>,
}

/// Configured MCP servers as seen from a directory (project-local config applies). Never starts
/// a server: `directory` is caller-chosen and its `mcp.json` defines commands.
pub async fn list_mcp(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<McpQuery>,
) -> impl IntoResponse {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    let config = state.config.read().await;
    let servers = state
        .mcp_manager
        .list_configured_statuses(&dir, &config)
        .await;
    Json(maukode_protocol::McpListResponse { servers })
}
