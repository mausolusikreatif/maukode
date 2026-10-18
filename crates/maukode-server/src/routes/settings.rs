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
use maukode_tools::SandboxMode;
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
    pub sandbox_mode: String,
    /// Shown for information; changing it requires editing the config file and restarting.
    pub bash_whitelist: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetDefaultPermissionModeRequest {
    pub permission_mode: PermissionMode,
}

#[derive(Debug, Deserialize)]
pub struct SetSandboxModeRequest {
    pub sandbox_mode: SandboxMode,
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
        sandbox_mode: serde_json::to_value(config.security.sandbox_mode)
            .unwrap_or_else(|_| serde_json::Value::String("path_jail".into()))
            .as_str()
            .unwrap_or("path_jail")
            .to_string(),
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

// ---------------------------------------------------------------------------------------------
// Where commands run (host or a lawang cell)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LawangSettingsView {
    pub url: String,
    /// Whether a token is stored. The token itself is never sent back.
    pub token_set: bool,
    /// `process`, `sandbox` or `vm`.
    pub isolation: String,
    pub image: Option<String>,
    /// MiB. `u32` on the wire so the TypeScript side gets a plain number, not a bigint.
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ExecutionSettings {
    /// `host` or `lawang`.
    pub backend: String,
    /// What a session is labelled with: `host`, `process`, `sandbox` or `vm`.
    pub label: String,
    pub lawang: LawangSettingsView,
    /// Why these settings cannot work, found without contacting the daemon.
    pub problems: Vec<String>,
}

fn execution_view(config: &maukode_core::ExecutionConfig) -> ExecutionSettings {
    ExecutionSettings {
        backend: match config.backend {
            maukode_core::ExecutionBackendKind::Host => "host",
            maukode_core::ExecutionBackendKind::Lawang => "lawang",
        }
        .to_string(),
        label: config.label().to_string(),
        lawang: LawangSettingsView {
            url: config.lawang.url.clone(),
            token_set: config
                .lawang
                .token
                .as_deref()
                .is_some_and(|t| !t.is_empty()),
            isolation: config.lawang.isolation.as_str().to_string(),
            image: config.lawang.image.clone(),
            memory_mb: config
                .lawang
                .memory_mb
                .map(|m| u32::try_from(m).unwrap_or(u32::MAX)),
        },
        problems: config.problems(),
    }
}

pub async fn get_execution(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    Json(execution_view(&state.config.read().await.execution))
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetLawangRequest {
    pub url: String,
    /// Absent keeps the stored token; an empty string clears it.
    #[serde(default)]
    pub token: Option<String>,
    pub isolation: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetExecutionRequest {
    /// `host` or `lawang`.
    pub backend: String,
    pub lawang: SetLawangRequest,
}

/// Choose where commands run. Applies from the next turn of every session.
pub async fn set_execution(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SetExecutionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let bad =
        |what: &str, value: &str| (StatusCode::BAD_REQUEST, format!("unknown {what} `{value}`"));
    let backend = match req.backend.as_str() {
        "host" => maukode_core::ExecutionBackendKind::Host,
        "lawang" => maukode_core::ExecutionBackendKind::Lawang,
        other => return Err(bad("backend", other)),
    };
    let isolation = match req.lawang.isolation.as_str() {
        "process" => maukode_tools::CellIsolation::Process,
        "sandbox" => maukode_tools::CellIsolation::Sandbox,
        "vm" => maukode_tools::CellIsolation::Vm,
        other => return Err(bad("isolation", other)),
    };
    let mut config = state.config.write().await;
    let token = match req.lawang.token {
        None => config.execution.lawang.token.clone(),
        Some(t) if t.is_empty() => None,
        Some(t) => Some(t),
    };
    let candidate = maukode_core::ExecutionConfig {
        backend,
        lawang: maukode_tools::LawangSettings {
            url: req.lawang.url.trim().to_string(),
            token,
            isolation,
            image: req
                .lawang
                .image
                .map(|i| i.trim().to_string())
                .filter(|i| !i.is_empty()),
            memory_mb: req.lawang.memory_mb.map(u64::from),
        },
    };
    // Only reject what would stop the lawang backend from working when it is the one chosen;
    // switching back to the host is always allowed.
    let problems = candidate.problems();
    if !problems.is_empty() {
        return Err((StatusCode::BAD_REQUEST, problems.join("; ")));
    }
    config.execution = candidate;
    config
        .save()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(execution_view(&config.execution)))
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LawangRuntimeStatus {
    pub id: String,
    pub available: bool,
    /// The isolation level this runtime provides, when available.
    pub isolation: Option<String>,
    /// Why it is unavailable.
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ExecutionStatus {
    /// `None` when the backend is the host and there is nothing to contact.
    pub reachable: Option<bool>,
    pub error: Option<String>,
    pub runtimes: Vec<LawangRuntimeStatus>,
}

/// Ask the lawang daemon what it can run, so the settings screen can say which isolation levels
/// exist on that machine instead of letting someone pick one that cannot start.
pub async fn execution_status(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let execution = state.config.read().await.execution.clone();
    if execution.backend != maukode_core::ExecutionBackendKind::Lawang {
        return Json(ExecutionStatus {
            reachable: None,
            error: None,
            runtimes: Vec::new(),
        });
    }
    let url = format!(
        "{}/v1/capabilities",
        execution.lawang.url.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let mut request = client.get(url);
    if let Some(token) = execution.lawang.token.as_deref().filter(|t| !t.is_empty()) {
        request = request.bearer_auth(token);
    }
    let failed = |error: String| ExecutionStatus {
        reachable: Some(false),
        error: Some(error),
        runtimes: Vec::new(),
    };
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => return Json(failed(format!("cannot reach the daemon: {e}"))),
    };
    match response.status().as_u16() {
        200 => {}
        401 => return Json(failed("the daemon rejected the token".to_string())),
        other => return Json(failed(format!("the daemon answered {other}"))),
    }
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
    let runtimes = body["runtimes"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|r| LawangRuntimeStatus {
                    id: r["id"].as_str().unwrap_or("?").to_string(),
                    available: r["status"] == "available",
                    isolation: r["capabilities"]["isolation"].as_str().map(str::to_string),
                    reason: r["reason"].as_str().map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default();
    Json(ExecutionStatus {
        reachable: Some(true),
        error: None,
        runtimes,
    })
}

pub async fn set_sandbox_mode(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SetSandboxModeRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if matches!(req.sandbox_mode, SandboxMode::Bubblewrap) && !SandboxMode::available() {
        return Err((
            StatusCode::BAD_REQUEST,
            "bubblewrap (bwrap) is not available on this host".into(),
        ));
    }
    let mut config = state.config.write().await;
    config.security.sandbox_mode = req.sandbox_mode;
    config
        .save()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "sandbox_mode": req.sandbox_mode }),
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
