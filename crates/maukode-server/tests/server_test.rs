use std::net::SocketAddr;
use std::time::Duration;
use maukode_server::{run_server, ServerClient, ServerConfig};
use tempfile::tempdir;

#[tokio::test]
async fn test_server_health_and_sessions() {
    let port = 14096;
    let bind_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let temp_dir = tempdir().unwrap();

    let server_config = ServerConfig {
        bind_addr,
        auth_token: Some("secret123".to_string()),
        default_dir: Some(temp_dir.path().to_path_buf()),
    };

    let _handle = tokio::spawn(async move {
        if let Err(e) = run_server(server_config).await {
            eprintln!("SERVER RUN ERROR: {e:?}");
        }
    });

    // Wait for server to start
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Test unauthorized client
    let unauth_client = ServerClient::new(format!("http://127.0.0.1:{port}"), None).unwrap();
    assert!(unauth_client.health().await.unwrap()); // health check is public
    let unauth_sessions = unauth_client.list_sessions(None, true).await;
    assert!(unauth_sessions.is_err()); // protected route fails without token!

    // Test authorized client
    let client = ServerClient::new(format!("http://127.0.0.1:{port}"), Some("secret123".to_string())).unwrap();
    assert!(client.health().await.unwrap());

    // Test create session with directory
    let meta = client.create_session(Some(temp_dir.path()), Some("Test Session")).await.unwrap();
    assert_eq!(meta.directory, temp_dir.path().to_string_lossy().to_string());

    // Test list sessions by directory
    let sessions = client.list_sessions(Some(temp_dir.path()), false).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, meta.id);
    assert_eq!(sessions[0].directory, temp_dir.path().to_string_lossy().to_string());

    // Test get session detail
    let detail = client.get_session(meta.id).await.unwrap();
    assert_eq!(detail.id, meta.id);
    assert_eq!(detail.directory, temp_dir.path().to_string_lossy().to_string());
}
