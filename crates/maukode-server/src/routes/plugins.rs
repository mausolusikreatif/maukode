use crate::state::ServerState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct PluginsQuery {
    pub directory: Option<String>,
}

pub async fn list_plugins(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<PluginsQuery>,
) -> impl IntoResponse {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    let mgr = state.plugin_manager.read().await;
    let plugins = mgr.list_plugins(Some(&dir));
    Json(plugins)
}

pub async fn get_plugin(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    Query(query): Query<PluginsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    let mgr = state.plugin_manager.read().await;
    mgr.get_plugin(&name, Some(&dir))
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Plugin '{name}' not found")))
}

pub async fn enable_plugin(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    Query(query): Query<PluginsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    let mut mgr = state.plugin_manager.write().await;
    let exists = mgr.get_plugin(&name, Some(&dir)).is_some();
    if !exists {
        return Err((StatusCode::NOT_FOUND, format!("Plugin '{name}' not found")));
    }
    mgr.set_plugin_enabled(&name, true);
    let updated = mgr.get_plugin(&name, Some(&dir));
    Ok(Json(updated))
}

pub async fn disable_plugin(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    Query(query): Query<PluginsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    let mut mgr = state.plugin_manager.write().await;
    let exists = mgr.get_plugin(&name, Some(&dir)).is_some();
    if !exists {
        return Err((StatusCode::NOT_FOUND, format!("Plugin '{name}' not found")));
    }
    mgr.set_plugin_enabled(&name, false);
    let updated = mgr.get_plugin(&name, Some(&dir));
    Ok(Json(updated))
}
