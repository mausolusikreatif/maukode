use crate::state::{PreviewProcess, ServerState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use maukode_protocol::{PreviewInfo, PreviewRequest, SessionId};
use maukode_tools::sandboxed_command;
use serde_json::Value;
use std::sync::Arc;

type ApiResult<T> = Result<T, (StatusCode, String)>;

fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, String) {
    (status, message.into())
}

async fn session_dir(state: &ServerState, id: SessionId) -> ApiResult<std::path::PathBuf> {
    let session = state
        .sessions
        .read()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Session not found"))?;
    let directory = session.read().await.directory.clone();
    Ok(directory)
}

pub async fn start_preview(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(request): Json<PreviewRequest>,
) -> ApiResult<impl IntoResponse> {
    let directory = session_dir(&state, id).await?;
    let package = directory.join("package.json");
    let package_text = tokio::fs::read_to_string(&package).await.map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "No package.json found; no safe preview command is configured",
        )
    })?;
    let package: Value = serde_json::from_str(&package_text)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "package.json is not valid JSON"))?;
    let scripts = package
        .get("scripts")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            error(
                StatusCode::BAD_REQUEST,
                "package.json has no scripts; no safe preview command is configured",
            )
        })?;
    let script = if scripts.contains_key("start") {
        "start"
    } else if scripts.contains_key("dev") {
        "dev"
    } else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "package.json has no start or dev script; no safe preview command is configured",
        ));
    };
    let mode = request
        .sandbox_mode
        .as_deref()
        .map(|s| serde_json::from_str(&format!("\"{s}\"")))
        .transpose()
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid sandbox_mode"))?
        .unwrap_or_default();
    let port = request.port.unwrap_or(3000);
    let mut command = sandboxed_command(
        "npm",
        &[
            "run".into(),
            script.into(),
            "--".into(),
            "--port".into(),
            port.to_string(),
        ],
        &directory,
        mode,
    )
    .map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    command.env("PORT", port.to_string()).kill_on_drop(true);
    let child = command.spawn().map_err(|e| {
        error(
            StatusCode::BAD_REQUEST,
            format!("Unable to start preview: {e}"),
        )
    })?;
    let info = PreviewInfo {
        status: "running".into(),
        port: Some(port),
        url: Some(format!("http://127.0.0.1:{port}")),
        ready: false,
        error: None,
    };
    let mut previews = state.previews.lock().await;
    if let Some(mut old) = previews.remove(&id) {
        let _ = old.child.kill().await;
    }
    previews.insert(
        id,
        PreviewProcess {
            child,
            info: info.clone(),
        },
    );
    Ok(Json(info))
}

pub async fn get_preview(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> ApiResult<impl IntoResponse> {
    let mut previews = state.previews.lock().await;
    let preview = previews
        .get_mut(&id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "No preview for session"))?;
    if let Some(status) = preview
        .child
        .try_wait()
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        preview.info.status = if status.success() {
            "stopped"
        } else {
            "failed"
        }
        .into();
        preview.info.error = Some(format!("preview exited with {status}"));
    }
    Ok(Json(preview.info.clone()))
}

pub async fn stop_preview(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> ApiResult<impl IntoResponse> {
    let mut previews = state.previews.lock().await;
    let mut preview = previews
        .remove(&id)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "No preview for session"))?;
    let _ = preview.child.kill().await;
    preview.info.status = "stopped".into();
    preview.info.ready = false;
    Ok(Json(preview.info))
}
