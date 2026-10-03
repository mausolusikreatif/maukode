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
