use maukode_server::{routes, ServerClient, ServerState};
use std::path::Path;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn start_test_server(directory: &Path) -> (String, tokio::task::JoinHandle<()>) {
    let state = Arc::new(
        ServerState::new(Some("secret123".to_string()), Some(directory.to_path_buf()))
            .await
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, routes::create_router(state))
            .await
            .unwrap();
    });
    (url, handle)
}

#[tokio::test]
async fn test_server_health_and_sessions() {
    let temp_dir = tempdir().unwrap();
    let (url, _handle) = start_test_server(temp_dir.path()).await;

    // Test unauthorized client
    let unauth_client = ServerClient::new(&url, None).unwrap();
    assert!(unauth_client.health().await.unwrap()); // health check is public
    let unauth_sessions = unauth_client.list_sessions(None, true).await;
    assert!(unauth_sessions.is_err()); // protected route fails without token!

    // Test authorized client
    let client = ServerClient::new(&url, Some("secret123".to_string())).unwrap();
    assert!(client.health().await.unwrap());

    // Test create session with directory
    let meta = client
        .create_session(Some(temp_dir.path()), Some("Test Session"))
        .await
        .unwrap();
    assert_eq!(
        meta.directory,
        temp_dir.path().to_string_lossy().to_string()
    );

    // Test list sessions by directory
    let sessions = client
        .list_sessions(Some(temp_dir.path()), false)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, meta.id);
    assert_eq!(
        sessions[0].directory,
        temp_dir.path().to_string_lossy().to_string()
    );

    // Test get session detail
    let detail = client.get_session(meta.id).await.unwrap();
    assert_eq!(detail.id, meta.id);
    assert_eq!(
        detail.directory,
        temp_dir.path().to_string_lossy().to_string()
    );
}

#[tokio::test]
async fn test_permission_endpoint() {
    let temp_dir = tempdir().unwrap();
    let (url, _handle) = start_test_server(temp_dir.path()).await;
    let client = ServerClient::new(&url, Some("secret123".to_string())).unwrap();
    let meta = client
        .create_session(Some(temp_dir.path()), None)
        .await
        .unwrap();

    // Responding to non-existent request should fail with 404
    let res = client
        .respond_permission(
            meta.id,
            "non_existent_req",
            maukode_protocol::PermissionDecision::AllowOnce,
        )
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_diff_and_undo_endpoints() {
    let temp_dir = tempdir().unwrap();
    let (url, _handle) = start_test_server(temp_dir.path()).await;
    let client = ServerClient::new(&url, Some("secret123".to_string())).unwrap();
    let meta = client
        .create_session(Some(temp_dir.path()), None)
        .await
        .unwrap();

    // 1. Initially diff should report clean / empty
    let diff = client.get_diff(meta.id).await.unwrap();
    assert_eq!(diff.files_changed, 0);

    // 2. Listing checkpoints initially should be empty
    let cps = client.list_checkpoints(meta.id).await.unwrap();
    assert!(cps.is_empty());

    // 3. Test codebase map endpoint
    tokio::fs::write(
        temp_dir.path().join("server_core.rs"),
        "pub struct AppServer { pub port: u16 }\nimpl AppServer { pub fn run(&self) {} }",
    )
    .await
    .unwrap();

    let map_resp = client
        .get_codebase_map(meta.id, None, None, Some(500))
        .await
        .unwrap();
    assert!(map_resp.map.contains("server_core.rs:"));
    assert!(map_resp.map.contains("pub struct AppServer"));

    // 4. Test init project endpoint
    let init_resp = client.init_project(meta.id, false).await.unwrap();
    assert!(init_resp.created);
    assert!(temp_dir.path().join("AGENTS.md").is_file());
    assert!(init_resp.content.contains("# Project:"));

    // 5. Test MCP servers list endpoint
    let mcp_resp = client.list_mcp_servers(meta.id).await.unwrap();
    // Initially empty unless configured
    assert!(mcp_resp.servers.is_empty());

    // 6. Test compaction endpoint
    let compact_resp = client
        .compact_session(meta.id, Some("Focus on server core"))
        .await
        .unwrap();
    assert_eq!(compact_resp.session_id, meta.id.to_string());
    assert!(compact_resp.compacted_messages <= 4);
}

#[tokio::test]
async fn test_desktop_backend_hardening() {
    let temp_dir = tempdir().unwrap();
    let (url, _handle) = start_test_server(temp_dir.path()).await;
    let http = reqwest::Client::new();
    let auth = "Bearer secret123";

    // Provider listing must never include stored API keys.
    let body: serde_json::Value = http
        .get(format!("{url}/api/v1/providers"))
        .header("authorization", auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for p in body["providers"].as_array().unwrap() {
        assert!(p["api_key"].is_null());
        assert!(p["has_api_key"].is_boolean());
    }

    // Permission mode is readable.
    let mode: serde_json::Value = http
        .get(format!("{url}/api/v1/permissions/mode"))
        .header("authorization", auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(mode["mode"].is_string());

    // Audit + delete lifecycle.
    let client = ServerClient::new(&url, Some("secret123".to_string())).unwrap();
    let meta = client
        .create_session(Some(temp_dir.path()), None)
        .await
        .unwrap();
    let audit = http
        .get(format!("{url}/api/v1/sessions/{}/audit", meta.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    assert!(audit.status().is_success());
    let del = http
        .delete(format!("{url}/api/v1/sessions/{}", meta.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success());
    let again = http
        .delete(format!("{url}/api/v1/sessions/{}", meta.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 404);
}

#[tokio::test]
async fn test_tokenless_server_rejects_foreign_origin() {
    let temp_dir = tempdir().unwrap();
    let state = Arc::new(
        ServerState::new(None, Some(temp_dir.path().to_path_buf()))
            .await
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, routes::create_router(state))
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    let evil = http
        .get(format!("{url}/api/v1/sessions?all=true"))
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(evil.status(), 403);
    let local = http
        .get(format!("{url}/api/v1/sessions?all=true"))
        .header("origin", "http://localhost:1420")
        .send()
        .await
        .unwrap();
    assert!(local.status().is_success());
}

#[tokio::test]
async fn test_sse_replay_session_mode_title_and_limits() {
    let temp_dir = tempdir().unwrap();
    let state = Arc::new(
        ServerState::new(
            Some("secret123".to_string()),
            Some(temp_dir.path().to_path_buf()),
        )
        .await
        .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app_state = Arc::clone(&state);
    tokio::spawn(async move {
        axum::serve(listener, routes::create_router(app_state))
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    let auth = "Bearer secret123";
    let client = ServerClient::new(&url, Some("secret123".to_string())).unwrap();

    // Titles: set on create, rename, clear.
    let a = client
        .create_session(Some(temp_dir.path()), Some("  Mine "))
        .await
        .unwrap();
    let b = client
        .create_session(Some(temp_dir.path()), None)
        .await
        .unwrap();
    let detail: serde_json::Value = http
        .get(format!("{url}/api/v1/sessions/{}", a.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["title"], "Mine");
    assert_eq!(detail["running"], false);
    http.patch(format!("{url}/api/v1/sessions/{}", a.id))
        .header("authorization", auth)
        .json(&serde_json::json!({ "title": "Renamed" }))
        .send()
        .await
        .unwrap();
    let listed = client
        .list_sessions(Some(temp_dir.path()), false)
        .await
        .unwrap();
    assert!(listed.iter().any(|s| s.title == "Renamed"));

    // Permission mode is isolated per session.
    http.post(format!("{url}/api/v1/permissions/mode"))
        .header("authorization", auth)
        .json(&serde_json::json!({ "mode": "trust", "session_id": a.id }))
        .send()
        .await
        .unwrap();
    let mode_of = |id: maukode_protocol::SessionId| {
        let http = http.clone();
        let url = url.clone();
        async move {
            let v: serde_json::Value = http
                .get(format!("{url}/api/v1/permissions/mode?session_id={id}"))
                .header("authorization", auth)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            v["mode"].as_str().unwrap().to_string()
        }
    };
    assert_eq!(mode_of(a.id).await, "trust");
    assert_ne!(mode_of(b.id).await, "trust");

    // SSE replay: events 1..=3 published, resume after 1 yields ids 2 and 3.
    for i in 0..3 {
        state
            .publish_event(
                a.id,
                maukode_protocol::LLMEvent::TextDelta {
                    text: format!("t{i}"),
                },
            )
            .await;
    }
    let mut resp = http
        .get(format!("{url}/api/v1/sessions/{}/events?since=1", a.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap();
    let mut body = String::new();
    while !(body.contains("id: 3") || body.contains("id:3")) {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
            .await
            .expect("sse timed out")
            .unwrap()
            .expect("stream ended");
        body.push_str(&String::from_utf8_lossy(&chunk));
    }
    assert!(body.contains("t1") && body.contains("t2") && !body.contains("t0"));
    let detail: serde_json::Value = http
        .get(format!("{url}/api/v1/sessions/{}", a.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["last_event_seq"], 3);

    // Agent limits are validated.
    let bad = http
        .post(format!("{url}/api/v1/config/agent"))
        .header("authorization", auth)
        .json(&serde_json::json!({ "max_steps": 0 }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);

    // Structured audit report is present.
    let audit: serde_json::Value = http
        .get(format!("{url}/api/v1/sessions/{}/audit", a.id))
        .header("authorization", auth)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(audit["report"]["mode_changes"].as_array().unwrap().len(), 1);
}
