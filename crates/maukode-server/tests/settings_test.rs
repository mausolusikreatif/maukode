//! Settings-related endpoints: ChatGPT sign-in state machine, providers, defaults, MCP.

mod common;

use maukode_server::{routes, ServerState};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpListener;

/// These tests share one config file (one `MAUKODE_HOME` per process) and each server writes its
/// whole in-memory config on save, so they must not interleave.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct App {
    url: String,
    http: reqwest::Client,
}

impl App {
    async fn start() -> Self {
        common::isolate_home();
        let dir = tempfile::tempdir().unwrap().keep();
        let state = Arc::new(ServerState::new(Some("t".into()), Some(dir)).await.unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, routes::create_router(state))
                .await
                .unwrap();
        });
        Self {
            url,
            http: reqwest::Client::new(),
        }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.url))
            .bearer_auth("t");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.unwrap();
        let status = res.status().as_u16();
        let text = res.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
}

use reqwest::Method as M;

#[tokio::test]
async fn chatgpt_sign_in_state_machine_without_touching_openai() {
    let _serial = SERIAL.lock().await;
    let app = App::start().await;

    let (s, v) = app.call(M::GET, "/api/v1/auth/chatgpt", None).await;
    assert_eq!(s, 200);
    assert!(v["email"].is_null() && v["accounts"].as_array().unwrap().is_empty());

    // Starting a sign-in returns the authorization URL (PKCE, loopback callback) and a handle.
    let (s, start) = app
        .call(M::POST, "/api/v1/auth/chatgpt/login", Some(json!({})))
        .await;
    assert_eq!(s, 200, "{start}");
    let url = reqwest::Url::parse(start["url"].as_str().unwrap()).unwrap();
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], "dynamic_agent_client");
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
    assert_eq!(start["browser_opened"], false);
    let id = start["attempt_id"].as_str().unwrap().to_string();

    let (_, st) = app
        .call(M::GET, &format!("/api/v1/auth/chatgpt/login/{id}"), None)
        .await;
    assert_eq!(st["state"], "pending");

    // A forged callback (wrong state) is rejected and nothing is saved.
    let forged = format!("{}?state=wrong&code=forged", q["redirect_uri"]);
    assert!(reqwest::get(forged).await.unwrap().status().is_success());
    let mut failed = Value::Null;
    for _ in 0..50 {
        let (_, st) = app
            .call(M::GET, &format!("/api/v1/auth/chatgpt/login/{id}"), None)
            .await;
        if st["state"] != "pending" {
            failed = st;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(failed["state"], "failed");
    assert!(failed["error"].as_str().unwrap().contains("state mismatch"));
    let (_, v) = app.call(M::GET, "/api/v1/auth/chatgpt", None).await;
    assert!(v["accounts"].as_array().unwrap().is_empty());

    // A new attempt replaces the old one; the old id is gone; cancelling frees the slot.
    let (_, second) = app
        .call(M::POST, "/api/v1/auth/chatgpt/login", Some(json!({})))
        .await;
    let id2 = second["attempt_id"].as_str().unwrap().to_string();
    assert_ne!(id, id2);
    let (s, _) = app
        .call(M::GET, &format!("/api/v1/auth/chatgpt/login/{id}"), None)
        .await;
    assert_eq!(s, 404);
    let (s, _) = app
        .call(
            M::DELETE,
            &format!("/api/v1/auth/chatgpt/login/{id2}"),
            None,
        )
        .await;
    assert_eq!(s, 200);
    let (s, _) = app
        .call(M::GET, &format!("/api/v1/auth/chatgpt/login/{id2}"), None)
        .await;
    assert_eq!(s, 404);

    // Without an account there is nothing to switch to or sign out of.
    let (s, _) = app
        .call(
            M::POST,
            "/api/v1/auth/chatgpt/account",
            Some(json!({ "client_id": "nope" })),
        )
        .await;
    assert_eq!(s, 404);
    let (s, _) = app.call(M::DELETE, "/api/v1/auth/chatgpt", None).await;
    assert_eq!(s, 400);
}

#[tokio::test]
async fn provider_keys_are_kept_cleared_and_removed_on_request() {
    let _serial = SERIAL.lock().await;
    let app = App::start().await;
    let provider = |key: Value| {
        json!({ "id": "extra", "name": "Extra", "kind": "open_ai_compatible",
                "base_url": "http://127.0.0.1:1", "api_key": key, "default_model": "m" })
    };
    let has_key = |list: &Value| {
        list["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "extra")
            .map(|p| p["has_api_key"].as_bool().unwrap())
    };

    app.call(
        M::POST,
        "/api/v1/providers",
        Some(provider(json!("secret"))),
    )
    .await;
    let (_, list) = app.call(M::GET, "/api/v1/providers", None).await;
    assert_eq!(has_key(&list), Some(true));
    assert!(
        !list.to_string().contains("secret"),
        "keys must never be returned"
    );

    app.call(M::POST, "/api/v1/providers", Some(provider(Value::Null)))
        .await; // keep
    let (_, list) = app.call(M::GET, "/api/v1/providers", None).await;
    assert_eq!(has_key(&list), Some(true));

    app.call(M::POST, "/api/v1/providers", Some(provider(json!(""))))
        .await; // clear
    let (_, list) = app.call(M::GET, "/api/v1/providers", None).await;
    assert_eq!(has_key(&list), Some(false));

    let active = list["active_provider"].as_str().unwrap().to_string();
    let (s, _) = app
        .call(M::DELETE, &format!("/api/v1/providers/{active}"), None)
        .await;
    assert_eq!(s, 409, "the active provider cannot be removed");
    let (s, _) = app.call(M::DELETE, "/api/v1/providers/extra", None).await;
    assert_eq!(s, 200);
    let (s, _) = app.call(M::DELETE, "/api/v1/providers/extra", None).await;
    assert_eq!(s, 404);
}

#[tokio::test]
async fn default_approval_mode_persists_and_applies_to_new_sessions_only() {
    let _serial = SERIAL.lock().await;
    let app = App::start().await;
    let (_, before) = app.call(M::GET, "/api/v1/config/security", None).await;
    assert!(before["bash_whitelist"].is_array() && before["jail_path_traversal"].is_boolean());

    let dir = tempfile::tempdir().unwrap();
    let (_, old) = app
        .call(
            M::POST,
            "/api/v1/sessions",
            Some(json!({ "directory": dir.path() })),
        )
        .await;
    let old_id = old["id"].as_str().unwrap().to_string();
    // Touch the session's own mode so it has its own manager before the default changes.
    app.call(
        M::GET,
        &format!("/api/v1/permissions/mode?session_id={old_id}"),
        None,
    )
    .await;

    let (s, _) = app
        .call(
            M::PUT,
            "/api/v1/config/security",
            Some(json!({ "permission_mode": "strict" })),
        )
        .await;
    assert_eq!(s, 200);
    let (_, now) = app.call(M::GET, "/api/v1/config/security", None).await;
    assert_eq!(now["permission_mode"], "strict");

    // Persisted to the config file, not just memory.
    let home = std::env::var("MAUKODE_HOME").unwrap();
    let saved = std::fs::read_to_string(format!("{home}/config/maukode/config.json")).unwrap();
    assert!(saved.contains("\"permission_mode\": \"strict\""), "{saved}");

    let (_, new) = app
        .call(
            M::POST,
            "/api/v1/sessions",
            Some(json!({ "directory": dir.path() })),
        )
        .await;
    let (_, mode) = app
        .call(
            M::GET,
            &format!(
                "/api/v1/permissions/mode?session_id={}",
                new["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(mode["mode"], "strict");
    let (_, mode) = app
        .call(
            M::GET,
            &format!("/api/v1/permissions/mode?session_id={old_id}"),
            None,
        )
        .await;
    assert_ne!(
        mode["mode"], "strict",
        "existing sessions keep their own mode"
    );

    let (s, v) = app.call(M::GET, "/api/v1/mcp", None).await;
    assert_eq!(s, 200);
    assert!(v["servers"].is_array());
}

/// A project's `.maukode/mcp.json` defines commands, so it is ignored until the user trusts the
/// folder. Even then, listing MCP servers is a GET and must never run the configured command;
/// only an agent turn or an explicit POST connects it.
#[tokio::test]
async fn listing_mcp_servers_never_spawns_them_but_connect_does() {
    let _serial = SERIAL.lock().await;
    let app = App::start().await;
    let project = tempfile::tempdir().unwrap();
    let marker = project.path().join("spawned");
    std::fs::create_dir_all(project.path().join(".maukode")).unwrap();
    std::fs::write(
        project.path().join(".maukode/mcp.json"),
        json!({"mcpServers": {"probe": {
            "command": "sh",
            "args": ["-c", format!("touch {}; sleep 1", marker.display())]
        }}})
        .to_string(),
    )
    .unwrap();

    let (s, v) = app
        .call(
            M::POST,
            "/api/v1/sessions",
            Some(json!({"directory": project.path()})),
        )
        .await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_str().unwrap().to_string();

    // Untrusted: the project's server is neither listed nor started, even on an explicit connect.
    let (s, v) = app
        .call(M::GET, &format!("/api/v1/sessions/{id}/mcp"), None)
        .await;
    assert_eq!(s, 200);
    let listed = v["servers"].as_array().unwrap();
    assert!(!listed.iter().any(|x| x["name"] == "probe"), "{v}");
    let (_, _) = app
        .call(M::POST, &format!("/api/v1/sessions/{id}/mcp"), None)
        .await;
    assert!(
        !marker.exists(),
        "an untrusted project must not start its MCP servers"
    );

    // `maukode trust` writes the folder to the configuration file.
    let mut config = maukode_core::AppConfig::load();
    config
        .security
        .trusted_projects
        .push(project.path().to_path_buf());
    config.save().unwrap();

    let (s, v) = app
        .call(M::GET, &format!("/api/v1/sessions/{id}/mcp"), None)
        .await;
    assert_eq!(s, 200);
    let probe = v["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "probe")
        .expect("configured server is listed");
    assert_eq!(probe["status"], "not_started");
    assert!(!marker.exists(), "GET must not spawn the server");

    let (_, _) = app.call(M::GET, "/api/v1/mcp", None).await;
    assert!(!marker.exists(), "GET /mcp must not spawn the server");

    let (s, _) = app
        .call(M::POST, &format!("/api/v1/sessions/{id}/mcp"), None)
        .await;
    assert_eq!(s, 200);
    assert!(marker.exists(), "explicit connect spawns the server");
}

#[tokio::test]
async fn a_daemon_saving_its_config_keeps_projects_trusted_while_it_ran() {
    let _serial = SERIAL.lock().await;
    let app = App::start().await;

    // `maukode trust` runs in another process while the daemon is up.
    let project = tempfile::tempdir().unwrap();
    let mut cli_config = maukode_core::AppConfig::load();
    cli_config
        .security
        .trusted_projects
        .push(project.path().to_path_buf());
    cli_config.save_trusted_projects().unwrap();

    // The daemon then saves its older in-memory configuration.
    let (s, _) = app
        .call(
            M::PUT,
            "/api/v1/config/security",
            Some(json!({ "permission_mode": "ask" })),
        )
        .await;
    assert_eq!(s, 200);
    assert!(maukode_core::AppConfig::is_trusted_project_on_disk(
        project.path()
    ));

    // Revoking works the same way.
    cli_config.security.trusted_projects.clear();
    cli_config.save_trusted_projects().unwrap();
    app.call(
        M::PUT,
        "/api/v1/config/security",
        Some(json!({ "permission_mode": "ask" })),
    )
    .await;
    assert!(!maukode_core::AppConfig::is_trusted_project_on_disk(
        project.path()
    ));
}
