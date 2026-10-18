//! Where a session's commands run, through the whole server: real HTTP server, real agent runner,
//! real tools, a scripted LLM gateway, and either a stand-in for the lawang daemon or (when
//! `LAWANGD_BIN` is set) a real one.
//!
//! The property that matters most: a command that was routed to lawang never runs on the host,
//! whatever goes wrong on the way.

mod common;

use common::{finish_chunk, start_gateway, text_chunk, tool_call_chunk, Gateway, SseReader, Step};
use maukode_server::{routes, ServerState};
use reqwest::Method as M;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "exec-token";

/// All tests here share one config file (one `MAUKODE_HOME` per process) and each server writes its
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
        let state = Arc::new(
            ServerState::new(Some(TOKEN.into()), Some(dir))
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
        Self {
            url,
            http: reqwest::Client::new(),
        }
    }

    async fn call(&self, method: M, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(TOKEN);
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

    async fn ok(&self, method: M, path: &str, body: Value) -> Value {
        let (status, v) = self.call(method, path, Some(body)).await;
        assert!((200..300).contains(&status), "{path}: {status} {v}");
        v
    }

    /// Point commands at a lawang daemon.
    async fn use_lawang(
        &self,
        url: &str,
        token: Option<&str>,
        isolation: &str,
        image: Option<&str>,
    ) -> (u16, Value) {
        self.call(
            M::PUT,
            "/api/v1/config/execution",
            Some(json!({
                "backend": "lawang",
                "lawang": { "url": url, "token": token, "isolation": isolation, "image": image, "memory_mb": null }
            })),
        )
        .await
    }

    async fn use_host(&self) {
        self.ok(
            M::PUT,
            "/api/v1/config/execution",
            json!({ "backend": "host", "lawang": { "url": "http://127.0.0.1:7777", "isolation": "process" } }),
        )
        .await;
    }

    async fn session_on(&self, gw: &Gateway, workdir: &Path) -> String {
        self.ok(
            M::POST,
            "/api/v1/providers",
            json!({ "id": "mock", "name": "Mock", "kind": "open_ai_compatible", "base_url": gw.url, "api_key": "k", "default_model": "m" }),
        )
        .await;
        let s = self
            .ok(
                M::POST,
                "/api/v1/sessions",
                json!({ "directory": workdir, "provider": "mock", "model": "m" }),
            )
            .await;
        let id = s["id"].as_str().unwrap().to_string();
        // Commands run without asking; what is being tested is where they run.
        self.ok(
            M::POST,
            "/api/v1/permissions/mode",
            json!({ "mode": "trust", "session_id": id }),
        )
        .await;
        id
    }

    async fn events(&self, path: &str) -> SseReader {
        let r = self
            .http
            .get(format!("{}{path}", self.url))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        SseReader::new(r)
    }
}

// --- a stand-in for the lawang daemon -----------------------------------------------------------

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: Option<String>,
    body: Value,
}

struct FakeLawang {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeLawang {
    /// `exec_reply` is what `exec` answers; the cell "exists" once it has been created.
    async fn start(exec_reply: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let log = log.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    let head_end = loop {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    while raw.len() < head_end + length {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                    }
                    let mut first = head.lines().next().unwrap_or("").split(' ');
                    let (method, path) = (
                        first.next().unwrap_or("").to_string(),
                        first.next().unwrap_or("").to_string(),
                    );
                    let auth = head.lines().find_map(|l| {
                        l.to_ascii_lowercase()
                            .starts_with("authorization:")
                            .then(|| {
                                l.split_once(':')
                                    .map(|x| x.1.trim().to_string())
                                    .unwrap_or_default()
                            })
                    });
                    let body = serde_json::from_slice(&raw[head_end..]).unwrap_or(Value::Null);
                    let created = {
                        let mut l = log.lock().unwrap();
                        let created = l
                            .iter()
                            .any(|s| s.method == "POST" && s.path == "/v1/cells");
                        l.push(Seen {
                            method: method.clone(),
                            path: path.clone(),
                            auth,
                            body,
                        });
                        created
                    };
                    let (status, reply) = match (method.as_str(), path.as_str()) {
                        ("GET", "/v1/capabilities") => (
                            200,
                            r#"{"runtimes":[
                                {"id":"bwrap","status":"available","capabilities":{"isolation":"process"}},
                                {"id":"gvisor","status":"unavailable","reason":"`runsc` not found on PATH"},
                                {"id":"firecracker","status":"unavailable","reason":"/dev/kvm: no such file"}]}"#
                                .to_string(),
                        ),
                        ("GET", p) if p.starts_with("/v1/cells/") && !created => (404, r#"{"error":"no such cell"}"#.into()),
                        ("GET", _) => (200, r#"{"isolation":"process","state":"running","spec":{"limits":{}}}"#.into()),
                        ("POST", "/v1/cells") => (201, "{}".into()),
                        ("POST", p) if p.ends_with("/exec") => (200, exec_reply.to_string()),
                        ("DELETE", _) => (204, String::new()),
                        _ => (500, r#"{"error":"unexpected"}"#.into()),
                    };
                    let out = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    let _ = stream.write_all(out.as_bytes()).await;
                });
            }
        });
        Self { url, seen }
    }

    fn calls(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// A turn that runs one bash command and then finishes.
async fn bash_turn(command: &str) -> Gateway {
    start_gateway(vec![
        Step::Sse(vec![
            tool_call_chunk("call_1", "bash", json!({ "command": command })),
            finish_chunk("tool_calls"),
        ]),
        Step::Sse(vec![text_chunk("done"), finish_chunk("stop")]),
    ])
    .await
}

async fn run_turn(app: &App, id: &str) -> Vec<Value> {
    let mut feed = app.events(&format!("/api/v1/sessions/{id}/events")).await;
    app.ok(
        M::POST,
        &format!("/api/v1/sessions/{id}/prompt"),
        json!({ "prompt": "go" }),
    )
    .await;
    feed.until("turn_stopped").await
}

fn tool_result_text(events: &[Value]) -> String {
    events
        .iter()
        .filter(|e| e["type"] == "tool_result")
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

// --- tests -----------------------------------------------------------------------------------

#[tokio::test]
async fn a_command_goes_to_the_lawang_cell_and_never_runs_on_the_host() {
    let _serial = SERIAL.lock().await;
    let work = tempfile::tempdir().unwrap();
    let marker = work.path().join("ran-on-the-host");
    let daemon =
        FakeLawang::start(r#"{"stdout":"from-the-cell\n","stderr":"","exit_code":0}"#).await;
    let app = App::start().await;
    app.use_lawang(&daemon.url, Some("secret-token"), "sandbox", None)
        .await;
    let gw = bash_turn(&format!("touch {}", marker.display())).await;
    let id = app.session_on(&gw, work.path()).await;

    let events = run_turn(&app, &id).await;
    assert_eq!(events.last().unwrap()["reason"], "completed", "{events:?}");
    assert!(
        tool_result_text(&events).contains("from-the-cell"),
        "the model must receive the cell's output"
    );
    assert!(
        !marker.exists(),
        "the command must not have run on the host"
    );

    let calls = daemon.calls();
    assert!(
        calls
            .iter()
            .all(|c| c.auth.as_deref() == Some("Bearer secret-token")),
        "{calls:?}"
    );
    let create = calls
        .iter()
        .find(|c| c.method == "POST" && c.path == "/v1/cells")
        .expect("a cell was created");
    assert_eq!(create.body["isolation"], "sandbox");
    assert_eq!(
        create.body["spec"]["workspace"]["host"],
        work.path().to_str().unwrap()
    );
    let exec = calls
        .iter()
        .find(|c| c.path.ends_with("/exec"))
        .expect("the command was sent");
    assert_eq!(exec.body["cmd"][4], format!("touch {}", marker.display()));
    assert_eq!(exec.body["cwd"], "/work");
    app.use_host().await;
}

#[tokio::test]
async fn with_the_host_backend_the_command_still_runs_locally() {
    let _serial = SERIAL.lock().await;
    let work = tempfile::tempdir().unwrap();
    let marker = work.path().join("ran-on-the-host");
    let app = App::start().await;
    app.use_host().await;
    let gw = bash_turn(&format!("touch {}", marker.display())).await;
    let id = app.session_on(&gw, work.path()).await;
    let events = run_turn(&app, &id).await;
    assert_eq!(events.last().unwrap()["reason"], "completed");
    assert!(marker.exists(), "the default behavior must not change");
}

#[tokio::test]
async fn an_unreachable_daemon_fails_the_command_and_nothing_runs_on_the_host() {
    let _serial = SERIAL.lock().await;
    let work = tempfile::tempdir().unwrap();
    let marker = work.path().join("ran-on-the-host");
    let app = App::start().await;
    app.use_lawang("http://127.0.0.1:1", None, "process", None)
        .await;
    let gw = bash_turn(&format!("touch {}", marker.display())).await;
    let id = app.session_on(&gw, work.path()).await;
    let events = run_turn(&app, &id).await;
    let results = tool_result_text(&events);
    assert!(
        results.contains("cannot reach the lawang daemon"),
        "{results}"
    );
    assert!(
        !marker.exists(),
        "a refused command must not fall back to the host"
    );
    app.use_host().await;
}

#[tokio::test]
async fn a_misconfigured_backend_stops_the_turn_before_anything_runs() {
    let _serial = SERIAL.lock().await;
    let work = tempfile::tempdir().unwrap();
    let marker = work.path().join("ran-on-the-host");
    common::isolate_home();
    // The API refuses to save a broken setting, so write one the way a hand-edited config file
    // could have it: a VM with no image.
    let mut config = maukode_core::AppConfig::load();
    config.execution = maukode_core::ExecutionConfig {
        backend: maukode_core::ExecutionBackendKind::Lawang,
        lawang: maukode_tools::LawangSettings {
            isolation: maukode_tools::CellIsolation::Vm,
            image: None,
            ..maukode_tools::LawangSettings::default()
        },
    };
    config.save().unwrap();

    let app = App::start().await;
    let (_, shown) = app.call(M::GET, "/api/v1/config/execution", None).await;
    assert!(
        shown["problems"].as_array().is_some_and(|p| !p.is_empty()),
        "the settings screen shows the problem: {shown}"
    );

    let gw = bash_turn(&format!("touch {}", marker.display())).await;
    let id = app.session_on(&gw, work.path()).await;
    let (status, body) = app
        .call(
            M::POST,
            &format!("/api/v1/sessions/{id}/prompt"),
            Some(json!({ "prompt": "go" })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert!(
        body.to_string().contains("misconfigured") && body.to_string().contains("needs an image"),
        "{body}"
    );
    assert!(
        !marker.exists(),
        "nothing may run on the host when the chosen backend cannot work"
    );
    app.use_host().await;
}

#[tokio::test]
async fn the_settings_routes_hide_the_token_and_keep_it_unless_told_otherwise() {
    let _serial = SERIAL.lock().await;
    let app = App::start().await;
    app.use_host().await;
    let (_, d) = app.call(M::GET, "/api/v1/config/execution", None).await;
    assert_eq!(
        (d["backend"].as_str(), d["label"].as_str()),
        (Some("host"), Some("host"))
    );

    let (status, v) = app
        .use_lawang(
            "http://127.0.0.1:7777",
            Some("s3cret"),
            "sandbox",
            Some(" alpine:3.20 "),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["label"], "sandbox");
    assert_eq!(v["lawang"]["token_set"], true);
    assert_eq!(v["lawang"]["image"], "alpine:3.20", "trimmed");
    assert!(
        !v.to_string().contains("s3cret"),
        "the token must never be sent back: {v}"
    );

    // No token in the request keeps the stored one.
    let (_, v) = app
        .use_lawang("http://127.0.0.1:7777", None, "process", None)
        .await;
    assert_eq!(v["lawang"]["token_set"], true);
    // An empty token clears it.
    let (_, v) = app
        .use_lawang("http://127.0.0.1:7777", Some(""), "process", None)
        .await;
    assert_eq!(v["lawang"]["token_set"], false);

    // Bad input is explained, not saved.
    let (status, _) = app
        .call(
            M::PUT,
            "/api/v1/config/execution",
            Some(json!({"backend":"cloud","lawang":{"url":"http://x","isolation":"process"}})),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = app.use_lawang("http://x", None, "kvm", None).await;
    assert_eq!(status, 400);
    let (status, body) = app.use_lawang("ftp://x", None, "process", None).await;
    assert_eq!(status, 400);
    assert!(body.to_string().contains("http"), "{body}");
    // Going back to the host is always allowed, whatever the lawang fields hold.
    let (status, v) = app
        .call(
            M::PUT,
            "/api/v1/config/execution",
            Some(json!({"backend":"host","lawang":{"url":"junk","isolation":"vm"}})),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["label"], "host");
}

#[tokio::test]
async fn the_status_route_reports_which_isolation_levels_the_daemon_has() {
    let _serial = SERIAL.lock().await;
    let daemon = FakeLawang::start("{}").await;
    let app = App::start().await;

    app.use_host().await;
    let (_, s) = app
        .call(M::GET, "/api/v1/config/execution/status", None)
        .await;
    assert!(
        s["reachable"].is_null(),
        "nothing to contact for the host: {s}"
    );

    app.use_lawang(&daemon.url, Some("t"), "process", None)
        .await;
    let (_, s) = app
        .call(M::GET, "/api/v1/config/execution/status", None)
        .await;
    assert_eq!(s["reachable"], true, "{s}");
    let runtimes = s["runtimes"].as_array().unwrap();
    let find = |id: &str| runtimes.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(find("bwrap")["available"], true);
    assert_eq!(find("bwrap")["isolation"], "process");
    assert_eq!(find("gvisor")["available"], false);
    assert!(find("gvisor")["reason"].as_str().unwrap().contains("runsc"));
    assert!(find("firecracker")["reason"]
        .as_str()
        .unwrap()
        .contains("kvm"));

    app.use_lawang("http://127.0.0.1:1", None, "process", None)
        .await;
    let (_, s) = app
        .call(M::GET, "/api/v1/config/execution/status", None)
        .await;
    assert_eq!(s["reachable"], false);
    assert!(s["error"].as_str().unwrap().contains("cannot reach"), "{s}");
    app.use_host().await;
}

#[tokio::test]
async fn deleting_a_session_removes_its_cell_from_the_daemon() {
    let _serial = SERIAL.lock().await;
    let work = tempfile::tempdir().unwrap();
    let daemon = FakeLawang::start(r#"{"stdout":"ok\n","stderr":"","exit_code":0}"#).await;
    let app = App::start().await;
    app.use_lawang(&daemon.url, None, "process", None).await;
    let gw = bash_turn("echo hi").await;
    let id = app.session_on(&gw, work.path()).await;
    run_turn(&app, &id).await;
    assert!(daemon
        .calls()
        .iter()
        .any(|c| c.method == "POST" && c.path == "/v1/cells"));

    app.ok(M::DELETE, &format!("/api/v1/sessions/{id}"), json!({}))
        .await;
    let deleted = daemon
        .calls()
        .iter()
        .filter(|c| c.method == "DELETE" && c.path.starts_with("/v1/cells/mk-"))
        .count();
    assert_eq!(
        deleted,
        1,
        "the cell must go with its session: {:?}",
        daemon.calls()
    );
    app.use_host().await;
}

#[tokio::test]
async fn a_session_keeps_one_cell_across_turns() {
    let _serial = SERIAL.lock().await;
    let work = tempfile::tempdir().unwrap();
    let daemon = FakeLawang::start(r#"{"stdout":"ok\n","stderr":"","exit_code":0}"#).await;
    let app = App::start().await;
    app.use_lawang(&daemon.url, None, "process", None).await;
    let gw = start_gateway(vec![
        Step::Sse(vec![
            tool_call_chunk("c1", "bash", json!({ "command": "echo 1" })),
            finish_chunk("tool_calls"),
        ]),
        Step::Sse(vec![
            tool_call_chunk("c2", "bash", json!({ "command": "echo 2" })),
            finish_chunk("tool_calls"),
        ]),
        Step::Sse(vec![text_chunk("done"), finish_chunk("stop")]),
    ])
    .await;
    let id = app.session_on(&gw, work.path()).await;
    run_turn(&app, &id).await;
    let calls = daemon.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.method == "POST" && c.path == "/v1/cells")
            .count(),
        1
    );
    assert_eq!(
        calls.iter().filter(|c| c.path.ends_with("/exec")).count(),
        2
    );
    app.use_host().await;
}

/// A real lawangd that is stopped (and waited for) however the test ends.
struct RealDaemon(std::process::Child);

impl Drop for RealDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The whole stack with a real daemon (bubblewrap), when one is available.
#[tokio::test]
async fn a_real_lawang_daemon_runs_the_sessions_commands_in_a_cell() {
    let _serial = SERIAL.lock().await;
    let Some(bin) = std::env::var("LAWANGD_BIN")
        .ok()
        .filter(|b| Path::new(b).exists())
    else {
        eprintln!("skipped: set LAWANGD_BIN to a lawangd binary");
        return;
    };
    let state = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(&bin)
        .args([
            "--state",
            state.path().to_str().unwrap(),
            "--listen",
            "tcp:127.0.0.1:0",
            "--token",
            "real",
        ])
        .env("XDG_RUNTIME_DIR", state.path().join("xdg"))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(child.stderr.take().unwrap()),
        &mut line,
    )
    .unwrap();
    let _daemon = RealDaemon(child);
    let url = format!("http://{}", line.trim().rsplit("tcp:").next().unwrap());

    let work = tempfile::tempdir().unwrap();
    let app = App::start().await;
    let (status, v) = app.use_lawang(&url, Some("real"), "process", None).await;
    assert_eq!(status, 200, "{v}");
    let (_, st) = app
        .call(M::GET, "/api/v1/config/execution/status", None)
        .await;
    if st["runtimes"].as_array().is_none_or(|r| {
        !r.iter()
            .any(|x| x["id"] == "bwrap" && x["available"] == true)
    }) {
        eprintln!("skipped: bubblewrap is not usable here: {st}");
        app.use_host().await;
        return;
    }
    let gw = bash_turn("echo from-the-cell > produced.txt && uname -s").await;
    let id = app.session_on(&gw, work.path()).await;
    let events = run_turn(&app, &id).await;
    assert_eq!(events.last().unwrap()["reason"], "completed", "{events:?}");
    assert_eq!(
        std::fs::read_to_string(work.path().join("produced.txt")).unwrap(),
        "from-the-cell\n"
    );
    assert!(tool_result_text(&events).contains("Linux"));
    app.use_host().await;
}
