mod common;

use maukode_server::{routes, ServerClient, ServerState};
use std::path::Path;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn start_test_server(directory: &Path) -> (String, tokio::task::JoinHandle<()>) {
    common::isolate_home();
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

    // Archiving is stored with the session and shows in the list.
    let http = reqwest::Client::new();
    let archive = |archived: bool| {
        http.put(format!("{url}/api/v1/sessions/{}/archive", meta.id))
            .bearer_auth("secret123")
            .json(&serde_json::json!({ "archived": archived }))
            .send()
    };
    assert_eq!(archive(true).await.unwrap().status(), 200);
    let sessions = client
        .list_sessions(Some(temp_dir.path()), false)
        .await
        .unwrap();
    assert!(sessions[0].archived);
    assert_eq!(archive(false).await.unwrap().status(), 200);
    let sessions = client
        .list_sessions(Some(temp_dir.path()), false)
        .await
        .unwrap();
    assert!(!sessions[0].archived);

    // Opening a file in the editor stays inside the workspace.
    let open = |path: &str| {
        http.post(format!("{url}/api/v1/sessions/{}/open", meta.id))
            .bearer_auth("secret123")
            .json(&serde_json::json!({ "path": path }))
            .send()
    };
    assert_eq!(open("../outside.txt").await.unwrap().status(), 400);
    assert_eq!(open("missing.txt").await.unwrap().status(), 404);
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
    common::isolate_home();
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
    common::isolate_home();
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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Harness {
    url: String,
    state: Arc<ServerState>,
    http: reqwest::Client,
}

impl Harness {
    async fn start(dir: &Path) -> Self {
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

    async fn get(&self, path: &str) -> serde_json::Value {
        let r = self
            .http
            .get(format!("{}{path}", self.url))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "GET {path}: {}", r.status());
        r.json().await.unwrap()
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: serde_json::Value,
    ) -> reqwest::Response {
        self.http
            .request(method, format!("{}{path}", self.url))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn create(&self, body: serde_json::Value) -> serde_json::Value {
        let r = self
            .send(reqwest::Method::POST, "/api/v1/sessions", body)
            .await;
        assert!(
            r.status().is_success(),
            "create: {}",
            r.text().await.unwrap()
        );
        r.json().await.unwrap()
    }
}

#[tokio::test]
async fn test_worktree_isolation_per_file_diff_merge_and_delete() {
    let repo = tempdir().unwrap();
    let r = repo.path();
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["config", "user.email", "t@t"]);
    git(r, &["config", "user.name", "t"]);
    std::fs::write(r.join("a.txt"), "one\n").unwrap();
    git(r, &["add", "-A"]);
    git(r, &["commit", "-q", "-m", "init"]);

    let h = Harness::start(r).await;
    let a = h
        .create(serde_json::json!({ "directory": r, "isolation": "worktree" }))
        .await;
    let b = h
        .create(serde_json::json!({ "directory": r, "isolation": "worktree" }))
        .await;
    let (a_id, b_id) = (
        a["id"].as_str().unwrap().to_string(),
        b["id"].as_str().unwrap().to_string(),
    );
    let a_dir = std::path::PathBuf::from(a["directory"].as_str().unwrap());
    let b_dir = std::path::PathBuf::from(b["directory"].as_str().unwrap());
    assert_ne!(a_dir, b_dir);
    assert_ne!(a_dir, r);
    assert!(a["worktree"]["branch"]
        .as_str()
        .unwrap()
        .starts_with("maukode/"));

    // Sessions stay grouped under the original project.
    let listed = h
        .get(&format!("/api/v1/sessions?directory={}", r.display()))
        .await;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(listed[0]["running"], false);

    // Simulate the pre-turn snapshot the runner takes, then an agent editing its own checkout.
    for (id, dir) in [(&a_id, &a_dir), (&b_id, &b_dir)] {
        let sid: maukode_protocol::SessionId =
            serde_json::from_value(serde_json::json!(id)).unwrap();
        h.state
            .checkpoint_manager_for(dir)
            .await
            .unwrap()
            .create_checkpoint(&sid.to_string(), 0, "baseline")
            .await
            .unwrap();
    }
    std::fs::write(a_dir.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(a_dir.join("new.txt"), "n\n").unwrap();

    // Diffs are scoped to each session's own workspace.
    let a_files = h.get(&format!("/api/v1/sessions/{a_id}/diff/files")).await;
    assert_eq!(a_files["files"].as_array().unwrap().len(), 2);
    let b_files = h.get(&format!("/api/v1/sessions/{b_id}/diff/files")).await;
    assert!(b_files["files"].as_array().unwrap().is_empty());
    let one = h
        .get(&format!("/api/v1/sessions/{a_id}/diff/file?path=a.txt"))
        .await;
    assert!(one["diff"].as_str().unwrap().contains("+two"));
    assert_eq!(std::fs::read_to_string(r.join("a.txt")).unwrap(), "one\n");

    // Merge brings the work into the project; worktree info is reported.
    let wt = h.get(&format!("/api/v1/sessions/{a_id}/worktree")).await;
    assert_eq!(wt["dirty"], true);
    let merged = h
        .send(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{a_id}/worktree/merge"),
            serde_json::json!({}),
        )
        .await;
    assert!(merged.status().is_success());
    let outcome: serde_json::Value = merged.json().await.unwrap();
    assert_eq!(outcome["result"], "merged");
    assert_eq!(
        std::fs::read_to_string(r.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );

    // Non-worktree sessions have no worktree.
    let plain = h.create(serde_json::json!({ "directory": r })).await;
    let res = h
        .http
        .get(format!(
            "{}/api/v1/sessions/{}/worktree",
            h.url,
            plain["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);

    // Delete removes the checkout but keeps the branch.
    let del = h
        .http
        .delete(format!("{}/api/v1/sessions/{b_id}", h.url))
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success());
    assert!(!b_dir.exists());
    let branch = b["worktree"]["branch"].as_str().unwrap();
    assert!(git(r, &["branch", "--list", branch]).contains("maukode/"));

    // Clean up the merged session's checkout and branch (query flags).
    let del_a = h
        .http
        .delete(format!(
            "{}/api/v1/sessions/{a_id}?delete_branch=true",
            h.url
        ))
        .send()
        .await
        .unwrap();
    assert!(del_a.status().is_success());
    assert!(!a_dir.exists());

    // Non-repo directories are rejected for worktree isolation.
    let plain_dir = tempdir().unwrap();
    let bad = h
        .send(
            reqwest::Method::POST,
            "/api/v1/sessions",
            serde_json::json!({ "directory": plain_dir.path(), "isolation": "worktree" }),
        )
        .await;
    assert_eq!(bad.status(), 400);
}

#[tokio::test]
async fn test_message_pagination_model_pinning_and_global_events() {
    let dir = tempdir().unwrap();
    let h = Harness::start(dir.path()).await;

    // Global feed: subscribe first, then see lifecycle events.
    let mut feed = h
        .http
        .get(format!("{}/api/v1/events", h.url))
        .send()
        .await
        .unwrap();
    let created = h
        .create(serde_json::json!({ "directory": dir.path() }))
        .await;
    let id = created["id"].as_str().unwrap().to_string();
    let mut body = String::new();
    while !body.contains("session_created") {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), feed.chunk())
            .await
            .expect("global feed timed out")
            .unwrap()
            .expect("feed ended");
        body.push_str(&String::from_utf8_lossy(&chunk));
    }
    let listing = h
        .http
        .get(format!(
            "{}/api/v1/sessions?directory={}",
            h.url,
            dir.path().display()
        ))
        .send()
        .await
        .unwrap();
    assert!(listing.headers().contains_key("x-event-seq"));

    // Pagination over 120 messages.
    let sid: maukode_protocol::SessionId = serde_json::from_value(serde_json::json!(id)).unwrap();
    {
        let session = h.state.get_or_load_session(sid).await.unwrap().unwrap();
        let mut s = session.write().await;
        for i in 0..120 {
            s.add_message(maukode_protocol::Message::user(format!("m{i}")));
        }
    }
    let page1 = h
        .get(&format!("/api/v1/sessions/{id}/messages?limit=50"))
        .await;
    assert_eq!(
        (page1["start"].as_u64(), page1["total"].as_u64()),
        (Some(70), Some(120))
    );
    assert_eq!(page1["messages"].as_array().unwrap().len(), 50);
    let page2 = h
        .get(&format!(
            "/api/v1/sessions/{id}/messages?before=70&limit=50"
        ))
        .await;
    assert_eq!(page2["start"], 20);
    let page3 = h
        .get(&format!(
            "/api/v1/sessions/{id}/messages?before=20&limit=50"
        ))
        .await;
    assert_eq!(
        (page3["start"].as_u64(), page3["has_more"].as_bool()),
        (Some(0), Some(false))
    );
    let tail = h.get(&format!("/api/v1/sessions/{id}?limit=10")).await;
    assert_eq!(tail["messages"].as_array().unwrap().len(), 10);
    assert_eq!(
        (
            tail["message_total"].as_u64(),
            tail["messages_start"].as_u64()
        ),
        (Some(120), Some(110))
    );
    let full = h.get(&format!("/api/v1/sessions/{id}")).await;
    assert_eq!(full["messages"].as_array().unwrap().len(), 120);

    // Model pinning is per session and validated.
    let other = h
        .create(serde_json::json!({ "directory": dir.path() }))
        .await;
    let ok = h
        .send(
            reqwest::Method::PUT,
            &format!("/api/v1/sessions/{id}/model"),
            serde_json::json!({ "provider_id": "ollama", "model": "pinned-model" }),
        )
        .await;
    assert!(ok.status().is_success());
    let pinned = h.get(&format!("/api/v1/sessions/{id}")).await;
    assert_eq!(
        (pinned["provider"].as_str(), pinned["model"].as_str()),
        (Some("ollama"), Some("pinned-model"))
    );
    let untouched = h
        .get(&format!(
            "/api/v1/sessions/{}",
            other["id"].as_str().unwrap()
        ))
        .await;
    assert_ne!(untouched["model"].as_str(), Some("pinned-model"));
    let bad = h
        .send(
            reqwest::Method::PUT,
            &format!("/api/v1/sessions/{id}/model"),
            serde_json::json!({ "provider_id": "nope", "model": "x" }),
        )
        .await;
    assert_eq!(bad.status(), 400);
    let bad_create = h
        .send(
            reqwest::Method::POST,
            "/api/v1/sessions",
            serde_json::json!({ "provider": "nope" }),
        )
        .await;
    assert_eq!(bad_create.status(), 400);
}

#[tokio::test]
async fn tokenless_server_rejects_dns_rebinding_hosts() {
    let temp_dir = tempdir().unwrap();
    common::isolate_home();
    let state = Arc::new(
        ServerState::new(None, Some(temp_dir.path().to_path_buf()))
            .await
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}");
    tokio::spawn(async move {
        axum::serve(listener, routes::create_router(state))
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    let get = |host: &str| {
        http.get(format!("{url}/api/v1/sessions?all=true"))
            .header("host", host.to_string())
            .send()
    };

    // A page on evil.example re-resolved to 127.0.0.1 is same-origin, so it sends no Origin on
    // GETs; the Host header is what gives it away.
    for host in [
        format!("evil.example:{port}"),
        "localhost.evil.example".to_string(),
        "127.0.0.1.evil.example".to_string(),
    ] {
        assert_eq!(get(&host).await.unwrap().status(), 403, "{host}");
    }
    for host in [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
        "localhost".to_string(),
        format!("LocalHost:{port}"),
        "localhost.".to_string(),
    ] {
        assert!(get(&host).await.unwrap().status().is_success(), "{host}");
    }
    // A local Origin does not excuse a foreign Host.
    let both = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/api/v1/sessions?all=true"))
        .header("origin", "http://localhost:1420")
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(both.status(), 403);
}

#[tokio::test]
async fn test_subagent_profiles_endpoint() {
    let temp_dir = tempdir().unwrap();
    let (url, _handle) = start_test_server(temp_dir.path()).await;
    let client = ServerClient::new(&url, Some("secret123".to_string())).unwrap();

    let profiles = client.list_subagent_profiles().await.unwrap();
    assert!(!profiles.is_empty());
    assert!(profiles.iter().any(|p| p.name == "codebase-researcher"));
    assert!(profiles.iter().any(|p| p.name == "security-auditor"));
}
