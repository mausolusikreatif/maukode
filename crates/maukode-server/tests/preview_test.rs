//! Integration tests for the preview start/stop/proxy endpoints.
//!
//! These tests spin up a mock HTTP "dev server" on a random port, register it as
//! the preview target for a session, and verify that the reverse proxy forwards
//! requests correctly — including stripping X-Frame-Options/CSP headers and
//! rewriting redirects.

mod common;

use maukode_server::{routes, ServerState};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;

/// Harness identical to the one in server_test.rs but kept local for independence.
struct Harness {
    url: String,
    state: Arc<ServerState>,
    http: reqwest::Client,
}

impl Harness {
    async fn start(dir: &std::path::Path) -> Self {
        common::isolate_home();
        let state = Arc::new(
            ServerState::new(None, Some(dir.to_path_buf()))
                .await
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Arc::clone(&state);
        tokio::spawn(async move {
            axum::serve(listener, routes::create_router(app))
                .await
                .unwrap();
        });
        Self {
            url,
            state,
            http: reqwest::Client::new(),
        }
    }

    async fn create(&self, dir: &std::path::Path) -> String {
        let r = self
            .http
            .post(format!("{}/api/v1/sessions", self.url))
            .json(&serde_json::json!({ "directory": dir }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "create: {}", r.status());
        let v: serde_json::Value = r.json().await.unwrap();
        v["id"].as_str().unwrap().to_string()
    }
}

/// Register a mock dev server on the session's preview slot by directly inserting
/// into the ServerState previews map. This avoids depending on `npm` being installed.
async fn inject_mock_preview(state: &ServerState, id: &str, port: u16) {
    use maukode_protocol::PreviewInfo;
    use maukode_server::state::PreviewProcess;

    let sid: maukode_protocol::SessionId = serde_json::from_value(serde_json::json!(id)).unwrap();

    // Spawn a dummy child process so the PreviewProcess.child field is valid.
    let mut cmd = tokio::process::Command::new("sleep");
    cmd.arg("300");
    let child = cmd.spawn().unwrap();

    let info = PreviewInfo {
        status: "running".into(),
        port: Some(port),
        url: Some(format!("/api/v1/sessions/{id}/preview/proxy")),
        ready: true,
        error: None,
    };

    state
        .previews
        .lock()
        .await
        .insert(sid, PreviewProcess { child, info });
}

#[tokio::test]
async fn test_proxy_forwards_html_and_strips_frame_headers() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    // Mock dev server: returns HTML with X-Frame-Options and CSP.
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            (
                [
                    ("x-frame-options", "DENY"),
                    ("content-security-policy", "frame-ancestors 'none'"),
                ],
                "<html><body>Hello from dev server</body></html>",
            )
        }),
    );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    // Give the mock server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let session_id = h.create(dir.path()).await;
    inject_mock_preview(&h.state, &session_id, mock_port).await;

    // GET through the proxy.
    let resp = h
        .http
        .get(format!(
            "{}/api/v1/sessions/{}/preview/proxy/",
            h.url, session_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // X-Frame-Options and CSP must be stripped.
    let h2 = resp.headers();
    assert!(h2.get("x-frame-options").is_none());
    assert!(h2.get("content-security-policy").is_none());
    let body = resp.text().await.unwrap();
    assert!(body.contains("Hello from dev server"));
}

#[tokio::test]
async fn test_proxy_rewrites_redirect_location() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    // Mock dev server: redirects from /old to /new.
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = axum::Router::new().route(
        "/old",
        axum::routing::get(|| async { axum::response::Redirect::to("/new") }),
    );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    // Give the mock server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let session_id = h.create(dir.path()).await;
    inject_mock_preview(&h.state, &session_id, mock_port).await;

    // Use a no-redirect client so we can inspect the Location header.
    let no_redirect = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = no_redirect
        .get(format!(
            "{}/api/v1/sessions/{}/preview/proxy/old",
            h.url, session_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 303);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    // Root-relative redirect must be rewritten to go through the proxy prefix.
    assert!(location.contains("/preview/proxy/new"));
}

#[tokio::test]
async fn test_proxy_returns_404_when_no_preview() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;
    let session_id = h.create(dir.path()).await;

    let resp = h
        .http
        .get(format!(
            "{}/api/v1/sessions/{}/preview/proxy/",
            h.url, session_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_preview_get_and_stop_lifecycle() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = axum::Router::new().route("/", axum::routing::get(|| async { "ok" }));
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    let session_id = h.create(dir.path()).await;
    inject_mock_preview(&h.state, &session_id, mock_port).await;

    // GET preview status.
    let info: serde_json::Value = h
        .http
        .get(format!("{}/api/v1/sessions/{}/preview", h.url, session_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["status"], "running");
    assert_eq!(info["port"], mock_port);
    assert_eq!(info["ready"], true);
    assert!(info["url"].as_str().unwrap().contains("/preview/proxy"));

    // Stop the preview.
    let stopped: serde_json::Value = h
        .http
        .delete(format!("{}/api/v1/sessions/{}/preview", h.url, session_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stopped["status"], "stopped");
    assert_eq!(stopped["ready"], false);

    // After stop, the preview is removed so GET returns 404.
    let after = h
        .http
        .get(format!("{}/api/v1/sessions/{}/preview", h.url, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(after.status(), 404);
}

#[tokio::test]
async fn test_proxy_strips_hop_by_hop_headers() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            // Use hop-by-hop headers that hyper won't reject when set manually.
            // `transfer-encoding` and `connection` are managed by the transport
            // layer, so we use `proxy-connection`, `keep-alive`, and `te` instead.
            (
                [
                    ("proxy-connection", "keep-alive"),
                    ("keep-alive", "timeout=5"),
                    ("te", "trailers"),
                ],
                "data",
            )
        }),
    );
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    // Give the mock server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let session_id = h.create(dir.path()).await;
    inject_mock_preview(&h.state, &session_id, mock_port).await;

    let resp = h
        .http
        .get(format!(
            "{}/api/v1/sessions/{}/preview/proxy/",
            h.url, session_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // Hop-by-hop headers must not be forwarded.
    assert!(resp.headers().get("proxy-connection").is_none());
    assert!(resp.headers().get("keep-alive").is_none());
    assert!(resp.headers().get("te").is_none());
}

#[tokio::test]
async fn test_preview_status_failed_when_child_exits_nonzero() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    // Spawn a child that immediately exits with a non-zero status.
    let mut cmd = tokio::process::Command::new("false");
    cmd.kill_on_drop(true);
    let child = cmd.spawn().unwrap();

    use maukode_protocol::PreviewInfo;
    use maukode_server::state::PreviewProcess;

    let session_id = h.create(dir.path()).await;
    let sid: maukode_protocol::SessionId =
        serde_json::from_value(serde_json::json!(session_id)).unwrap();

    let info = PreviewInfo {
        status: "starting".into(),
        port: Some(12345),
        url: Some(format!("/api/v1/sessions/{sid}/preview/proxy")),
        ready: false,
        error: None,
    };

    h.state
        .previews
        .lock()
        .await
        .insert(sid, PreviewProcess { child, info });

    // Wait briefly for the child to exit, then query status.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let resp = h
        .http
        .get(format!("{}/api/v1/sessions/{}/preview", h.url, sid))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "failed");
    assert_eq!(body["ready"], false);
    assert!(body["error"].as_str().unwrap().contains("exited"));
}

#[tokio::test]
async fn test_preview_health_check_transitions_starting_to_running() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    // Mock dev server that accepts connections.
    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_port = mock_listener.local_addr().unwrap().port();
    let mock_app = axum::Router::new().route("/", axum::routing::get(|| async { "ok" }));
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    let session_id = h.create(dir.path()).await;

    // Inject a preview in "starting" state with a live child and a real listening port.
    use maukode_protocol::PreviewInfo;
    use maukode_server::state::PreviewProcess;

    let sid: maukode_protocol::SessionId =
        serde_json::from_value(serde_json::json!(session_id)).unwrap();

    let mut cmd = tokio::process::Command::new("sleep");
    cmd.arg("300");
    let child = cmd.spawn().unwrap();

    let info = PreviewInfo {
        status: "starting".into(),
        port: Some(mock_port),
        url: Some(format!("/api/v1/sessions/{sid}/preview/proxy")),
        ready: false,
        error: None,
    };

    h.state
        .previews
        .lock()
        .await
        .insert(sid, PreviewProcess { child, info });

    // Give the mock server a moment to start accepting connections.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // GET preview status — the health check should detect the listening dev server
    // and flip the status to "running" with ready=true.
    let resp = h
        .http
        .get(format!("{}/api/v1/sessions/{}/preview", h.url, sid))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "running");
    assert_eq!(body["ready"], true);
}
