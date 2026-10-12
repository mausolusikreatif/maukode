use crate::state::{PreviewProcess, ServerState};
use axum::{
    body::Body,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        FromRequest, Path, Request, State,
    },
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::{SinkExt, StreamExt as _};
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
        status: "starting".into(),
        port: Some(port),
        url: Some(format!("/api/v1/sessions/{id}/preview/proxy")),
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

    // Check whether the child process has exited.
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
        if !status.success() {
            preview.info.error = Some(format!("preview exited with {status}"));
        }
        preview.info.ready = false;
    } else if preview.info.status == "starting" {
        // The process is still alive — probe the port to see if the dev server
        // is now accepting connections. This is a cheap TCP connect with a short
        // timeout; it runs only while status is "starting" and stops once the
        // server responds (status flips to "running").
        if let Some(port) = preview.info.port {
            let addr = format!("127.0.0.1:{port}");
            match tokio::time::timeout(
                std::time::Duration::from_millis(500),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            {
                Ok(Ok(_)) => {
                    preview.info.status = "running".into();
                    preview.info.ready = true;
                }
                _ => {} // still starting — leave status as "starting"
            }
        }
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

/// Reverse-proxy handler that forwards an incoming request to the session's running
/// dev-server preview process. This lets a web UI embed the preview in an iframe
/// without cross-origin restrictions, and makes remote previews accessible when the
/// daemon runs on a VPS.
///
/// The path after `/api/v1/sessions/{id}/preview/proxy` is forwarded verbatim to the
/// dev server listening on `127.0.0.1:{port}`.
///
/// Reverse-proxy handler for the root path (`/preview/proxy` with no trailing path).
pub async fn proxy_preview_root(
    state: State<Arc<ServerState>>,
    id: Path<SessionId>,
    req: Request,
) -> Result<Response, (StatusCode, String)> {
    proxy_preview_inner(state.0, *id, req).await
}

/// Reverse-proxy handler for sub-paths (`/preview/proxy/{*rest}`).
pub async fn proxy_preview_path(
    state: State<Arc<ServerState>>,
    params: Path<(SessionId, String)>,
    req: Request,
) -> Result<Response, (StatusCode, String)> {
    proxy_preview_inner(state.0, params.0 .0, req).await
}

async fn proxy_preview_inner(
    state: Arc<ServerState>,
    id: SessionId,
    req: Request,
) -> Result<Response, (StatusCode, String)> {
    // Check whether this is a WebSocket upgrade request by inspecting headers.
    // We cannot use `WebSocketUpgrade` as an extractor alongside `Request` because
    // both consume the request body in axum 0.8.
    let is_ws_upgrade = req
        .headers()
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        && req
            .headers()
            .get(axum::http::header::CONNECTION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("upgrade"));

    if is_ws_upgrade {
        // Capture the path before the request is consumed by WebSocketUpgrade extraction.
        let path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_default();
        let ws = WebSocketUpgrade::from_request(req, &())
            .await
            .map_err(|e| error(StatusCode::BAD_REQUEST, format!("ws upgrade failed: {e}")))?;
        return proxy_preview_ws_inner(state, id, ws, &path).await;
    }

    // Otherwise, handle as a normal HTTP reverse-proxy request.
    // Look up the preview port without holding the lock longer than necessary.
    let port = {
        let previews = state.previews.lock().await;
        let preview = previews
            .get(&id)
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "No preview for session"))?;
        preview
            .info
            .port
            .ok_or_else(|| error(StatusCode::CONFLICT, "Preview has no port assigned"))?
    };

    // Build the upstream URL: take everything after `/proxy` in the path,
    // plus the original query string.
    let (parts, body) = req.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let forward_path = strip_proxy_prefix(path_and_query, &id);
    let upstream_url = format!("http://127.0.0.1:{port}{forward_path}");

    // Collect the request body for forwarding (limit to 16 MiB).
    let body_bytes = axum::body::to_bytes(body, 16 * 1024 * 1024)
        .await
        .map_err(|e| error(StatusCode::BAD_REQUEST, format!("failed to read body: {e}")))?;

    // Build the proxied request, preserving method and most headers.
    let method = parts.method.clone();
    let mut upstream_req = state
        .proxy_client
        .request(method, &upstream_url)
        .body(body_bytes);

    // Forward headers, excluding hop-by-hop headers that must not be proxied.
    for (name, value) in parts.headers.iter() {
        let name_lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&name_lower.as_str()) {
            continue;
        }
        upstream_req = upstream_req.header(name.clone(), value.clone());
    }

    let upstream_resp = upstream_req
        .send()
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, format!("preview unreachable: {e}")))?;

    // Build the response, streaming the body back to the client.
    let status = upstream_resp.status();
    let mut response_builder = Response::builder().status(status);

    // Copy response headers, excluding hop-by-hop headers, rewriting redirects,
    // and stripping frame-blocking headers so the preview can be embedded in an iframe.
    for (name, value) in upstream_resp.headers().iter() {
        let name_lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&name_lower.as_str()) {
            continue;
        }
        // Strip headers that would prevent iframe embedding.
        if STRIP_FOR_FRAME.contains(&name_lower.as_str()) {
            continue;
        }
        if name == axum::http::header::LOCATION {
            if let Ok(loc) = value.to_str() {
                let rewritten = rewrite_redirect(loc, port, &id);
                response_builder = response_builder.header(
                    name.clone(),
                    HeaderValue::from_str(&rewritten).unwrap_or_else(|_| value.clone()),
                );
                continue;
            }
        }
        response_builder = response_builder.header(name.clone(), value.clone());
    }

    // Stream the body chunks back to the client.
    let stream = upstream_resp.bytes_stream().map(|result| {
        result.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("stream error: {e}"))
        })
    });

    response_builder
        .body(Body::from_stream(stream))
        .map_err(|e| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("proxy body error: {e}"),
            )
        })
}

/// Hop-by-hop headers that must not be forwarded by a proxy (RFC 7230 §6.1).
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "host",
];

/// Headers that prevent iframe embedding and must be stripped so the preview
/// can be displayed in a Lovable-style split-pane web UI.
const STRIP_FOR_FRAME: &[&str] = &["x-frame-options", "content-security-policy"];

/// Extract the path (plus optional `?query`) that should be forwarded to the upstream
/// dev server.
///
/// The full matched path looks like `/api/v1/sessions/{id}/preview/proxy/{rest}`.
/// We need to isolate `{rest}` (including a leading `/`).
fn strip_proxy_prefix(path_and_query: &str, id: &SessionId) -> String {
    let prefix = format!("/api/v1/sessions/{id}/preview/proxy");
    match path_and_query.strip_prefix(&prefix) {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => "/".to_string(),
    }
}

/// Rewrite an absolute or root-relative redirect Location header so that it points
/// back through the proxy path prefix instead of the raw dev server URL.
fn rewrite_redirect(location: &str, port: u16, id: &SessionId) -> String {
    let raw = format!("http://127.0.0.1:{port}");
    let prefix = format!("/api/v1/sessions/{id}/preview/proxy");
    if let Some(rest) = location.strip_prefix(&raw) {
        if rest.is_empty() {
            format!("{prefix}/")
        } else {
            format!("{prefix}{rest}")
        }
    } else if location.starts_with('/') {
        format!("{prefix}{location}")
    } else {
        location.to_string()
    }
}

// ── WebSocket proxy (HMR support) ────────────────────────────────────────────

/// WebSocket upgrade handler for the proxy path.
///
/// Dev servers like Vite use WebSocket connections for hot-module replacement (HMR).
/// Without proxying these, live reload silently stops working in the iframe.
/// This handler bridges the client WebSocket to the upstream dev server WebSocket
/// at the same path, forwarding messages bidirectionally.
async fn proxy_preview_ws_inner(
    state: Arc<ServerState>,
    id: SessionId,
    ws: WebSocketUpgrade,
    original_path: &str,
) -> Result<Response, (StatusCode, String)> {
    // Look up the preview port.
    let port = {
        let previews = state.previews.lock().await;
        let preview = previews
            .get(&id)
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "No preview for session"))?;
        preview
            .info
            .port
            .ok_or_else(|| error(StatusCode::CONFLICT, "Preview has no port assigned"))?
    };

    // Build the upstream WS URL from the request path.
    let forward_path = strip_proxy_prefix(original_path, &id);
    let upstream_url = format!("ws://127.0.0.1:{port}{forward_path}");

    // Connect to the upstream dev server WebSocket.
    let (upstream, _resp) = tokio_tungstenite::connect_async(&upstream_url)
        .await
        .map_err(|e| {
            error(
                StatusCode::BAD_GATEWAY,
                format!("ws upstream connect failed: {e}"),
            )
        })?;

    // Accept the client WebSocket and bridge the two connections.
    Ok(ws.on_upgrade(move |client_ws| async move {
        bridge_websockets(client_ws, upstream).await;
    }))
}

/// Bidirectionally bridge two WebSocket connections until either side closes.
async fn bridge_websockets(
    client: WebSocket,
    upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

    // client → upstream
    let client_to_upstream = tokio::spawn(async move {
        while let Some(msg) = client_rx.next().await {
            let tungstenite_msg = match msg {
                Ok(WsMessage::Text(t)) => TungsteniteMessage::Text(t.to_string().into()),
                Ok(WsMessage::Binary(b)) => TungsteniteMessage::Binary(b.to_vec().into()),
                Ok(WsMessage::Ping(b)) => TungsteniteMessage::Ping(b.to_vec().into()),
                Ok(WsMessage::Pong(b)) => TungsteniteMessage::Pong(b.to_vec().into()),
                Ok(WsMessage::Close(_)) => {
                    let _ = upstream_tx.send(TungsteniteMessage::Close(None)).await;
                    break;
                }
                Err(_) => break,
            };
            if upstream_tx.send(tungstenite_msg).await.is_err() {
                break;
            }
        }
    });

    // upstream → client
    let upstream_to_client = tokio::spawn(async move {
        while let Some(msg) = upstream_rx.next().await {
            let axum_msg = match msg {
                Ok(TungsteniteMessage::Text(t)) => WsMessage::Text(t.to_string().into()),
                Ok(TungsteniteMessage::Binary(b)) => WsMessage::Binary(b.to_vec().into()),
                Ok(TungsteniteMessage::Ping(b)) => WsMessage::Ping(b.to_vec().into()),
                Ok(TungsteniteMessage::Pong(b)) => WsMessage::Pong(b.to_vec().into()),
                Ok(TungsteniteMessage::Close(_)) => {
                    let _ = client_tx.send(WsMessage::Close(None)).await;
                    break;
                }
                Ok(_) => continue, // frame-level messages we don't proxy
                Err(_) => break,
            };
            if client_tx.send(axum_msg).await.is_err() {
                break;
            }
        }
    });

    // Wait for either direction to finish.
    let _ = tokio::join!(client_to_upstream, upstream_to_client);
}
