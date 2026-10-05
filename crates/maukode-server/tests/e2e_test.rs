//! End-to-end: real HTTP server + real agent runner + real tools, against a scripted
//! OpenAI-compatible gateway. No live LLM account is needed.

mod common;

use common::{finish_chunk, start_gateway, text_chunk, tool_call_chunk, Gateway, SseReader, Step};
use maukode_server::{routes, ServerState};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;

const TOKEN: &str = "e2e-token";

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

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(TOKEN)
    }

    async fn json(&self, method: reqwest::Method, path: &str, body: Value) -> Value {
        let r = self.req(method, path).json(&body).send().await.unwrap();
        let status = r.status();
        let text = r.text().await.unwrap();
        assert!(status.is_success(), "{path}: {status} {text}");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    async fn get(&self, path: &str) -> Value {
        let r = self.req(reqwest::Method::GET, path).send().await.unwrap();
        assert!(r.status().is_success(), "GET {path}: {}", r.status());
        r.json().await.unwrap()
    }

    async fn events(&self, path: &str) -> SseReader {
        let r = self.req(reqwest::Method::GET, path).send().await.unwrap();
        assert!(r.status().is_success());
        SseReader::new(r)
    }

    /// Register the gateway as a provider and create a session pinned to it.
    async fn session_on(&self, gw: &Gateway, workdir: &Path) -> String {
        self.json(
            reqwest::Method::POST,
            "/api/v1/providers",
            json!({
                "id": "mock", "name": "Mock", "kind": "open_ai_compatible",
                "base_url": gw.url, "api_key": "k", "default_model": "m"
            }),
        )
        .await;
        let s = self
            .json(
                reqwest::Method::POST,
                "/api/v1/sessions",
                json!({ "directory": workdir, "provider": "mock", "model": "m" }),
            )
            .await;
        s["id"].as_str().unwrap().to_string()
    }

    async fn prompt(&self, id: &str, text: &str) {
        self.json(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{id}/prompt"),
            json!({ "prompt": text }),
        )
        .await;
    }
}

fn types(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("?").to_string())
        .collect()
}

#[tokio::test]
async fn tool_turn_with_permission_prompt_completes_even_if_gateway_says_stop() {
    let work = tempfile::tempdir().unwrap();
    // Step 1 reports finish_reason "stop" although it carries a tool call (gateway quirk).
    let gw = start_gateway(vec![
        Step::Sse(vec![
            tool_call_chunk(
                "call_1",
                "write_file",
                json!({ "path": "out.txt", "content": "hello" }),
            ),
            finish_chunk("stop"),
        ]),
        Step::Sse(vec![text_chunk("all done"), finish_chunk("stop")]),
    ])
    .await;
    let app = App::start().await;
    let id = app.session_on(&gw, work.path()).await;

    // Strict mode: every tool call needs approval.
    app.json(
        reqwest::Method::POST,
        "/api/v1/permissions/mode",
        json!({ "mode": "strict", "session_id": id }),
    )
    .await;

    let mut global = app.events("/api/v1/events").await;
    let mut feed = app.events(&format!("/api/v1/sessions/{id}/events")).await;
    app.prompt(&id, "write a file").await;

    // The prompt is visible to a client that connects mid-turn via the snapshot.
    let before = feed.until("permission_request").await;
    let request_id = before.last().unwrap()["request_id"]
        .as_str()
        .unwrap()
        .to_string();
    let snapshot = app.get(&format!("/api/v1/sessions/{id}")).await;
    assert_eq!(snapshot["running"], true);
    assert_eq!(snapshot["pending_permissions"][0]["request_id"], request_id);
    let listing = app.get("/api/v1/sessions?all=true").await;
    let mine = listing
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .unwrap();
    assert_eq!(mine["pending_permission_count"], 1);

    app.json(
        reqwest::Method::POST,
        &format!("/api/v1/sessions/{id}/permissions/{request_id}"),
        json!({ "decision": "allow_once" }),
    )
    .await;

    let after = feed.until("turn_stopped").await;
    let all: Vec<String> = types(&before).into_iter().chain(types(&after)).collect();
    assert!(all.contains(&"tool_call_finish".to_string()));
    assert!(all.contains(&"tool_result".to_string()));
    assert!(all.contains(&"text_delta".to_string()));
    let stop = after.last().unwrap();
    assert_eq!(stop["reason"], "completed", "events: {all:?}");
    assert_eq!(
        all.iter().filter(|t| *t == "turn_stopped").count(),
        1,
        "exactly one terminator"
    );

    // The tool really ran in the session's directory.
    assert_eq!(
        std::fs::read_to_string(work.path().join("out.txt")).unwrap(),
        "hello"
    );

    // The model saw the tool result on its second request.
    let requests = gw.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let second = requests[1]["messages"].as_array().unwrap();
    assert!(second.iter().any(|m| m["role"] == "tool"));
    assert_eq!(requests[0]["model"], "m");

    // State settles: not running, prompts cleared, history stored.
    let done = app.get(&format!("/api/v1/sessions/{id}")).await;
    assert_eq!(done["running"], false);
    assert!(done["pending_permissions"].as_array().unwrap().is_empty());
    assert_eq!(done["messages"].as_array().unwrap().len(), 4);

    // Dashboard feed saw the whole lifecycle.
    let seen = global.until("turn_finished").await;
    let t = types(&seen);
    assert!(t.contains(&"turn_started".to_string()));
    assert!(t.contains(&"permission_requested".to_string()));
    assert!(t.contains(&"permission_resolved".to_string()));
    assert_eq!(seen.last().unwrap()["reason"], "completed");
}

#[tokio::test]
async fn truncated_response_is_continued_automatically() {
    let work = tempfile::tempdir().unwrap();
    let gw = start_gateway(vec![
        Step::Sse(vec![text_chunk("part one, "), finish_chunk("length")]),
        Step::Sse(vec![text_chunk("part two"), finish_chunk("stop")]),
    ])
    .await;
    let app = App::start().await;
    let id = app.session_on(&gw, work.path()).await;

    let mut feed = app.events(&format!("/api/v1/sessions/{id}/events")).await;
    app.prompt(&id, "write a long answer").await;
    let events = feed.until("turn_stopped").await;
    assert_eq!(events.last().unwrap()["reason"], "completed");

    let requests = gw.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let last_user = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap();
    assert!(last_user["content"].as_str().unwrap().contains("cut off"));
}

#[tokio::test]
async fn provider_failure_is_reported_and_session_stays_usable() {
    let work = tempfile::tempdir().unwrap();
    let gw = start_gateway(vec![
        Step::Status(500),
        Step::Sse(vec![text_chunk("recovered"), finish_chunk("stop")]),
    ])
    .await;
    let app = App::start().await;
    let id = app.session_on(&gw, work.path()).await;

    let mut feed = app.events(&format!("/api/v1/sessions/{id}/events")).await;
    app.prompt(&id, "first").await;
    let events = feed.until("turn_stopped").await;
    let t = types(&events);
    assert!(t.contains(&"error".to_string()), "events: {t:?}");
    assert_eq!(events.last().unwrap()["reason"], "error");
    assert!(events.last().unwrap()["message"].is_string());

    // The turn slot was released: a retry is accepted (not 409) and succeeds.
    let detail = app.get(&format!("/api/v1/sessions/{id}")).await;
    assert_eq!(detail["running"], false);
    app.prompt(&id, "second").await;
    let events = feed.until("turn_stopped").await;
    assert_eq!(events.last().unwrap()["reason"], "completed");
}

#[tokio::test]
async fn concurrent_prompt_is_rejected_with_conflict() {
    let work = tempfile::tempdir().unwrap();
    // Strict mode keeps the first turn parked on a permission prompt.
    let gw = start_gateway(vec![
        Step::Sse(vec![
            tool_call_chunk("c", "write_file", json!({ "path": "a", "content": "b" })),
            finish_chunk("tool_calls"),
        ]),
        Step::Sse(vec![text_chunk("fine"), finish_chunk("stop")]),
    ])
    .await;
    let app = App::start().await;
    let id = app.session_on(&gw, work.path()).await;
    app.json(
        reqwest::Method::POST,
        "/api/v1/permissions/mode",
        json!({ "mode": "strict", "session_id": id }),
    )
    .await;
    let mut feed = app.events(&format!("/api/v1/sessions/{id}/events")).await;
    app.prompt(&id, "go").await;
    feed.until("permission_request").await;

    let second = app
        .req(
            reqwest::Method::POST,
            &format!("/api/v1/sessions/{id}/prompt"),
        )
        .json(&json!({ "prompt": "again" }))
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 409);

    // Cancelling unblocks it and closes the turn with `cancelled`.
    app.json(
        reqwest::Method::POST,
        &format!("/api/v1/sessions/{id}/cancel"),
        json!({}),
    )
    .await;
    let rest = feed.until("turn_stopped").await;
    assert_eq!(rest.last().unwrap()["reason"], "cancelled");
    assert!(
        !work.path().join("a").exists(),
        "cancelled tool must not run"
    );

    // History stays valid: the dangling tool call got a result, so the next request is accepted.
    app.prompt(&id, "continue").await;
    let rest = feed.until("turn_stopped").await;
    assert_eq!(rest.last().unwrap()["reason"], "completed");
    let requests = gw.requests.lock().unwrap().clone();
    let tool_msg = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "tool")
        .expect("cancelled tool call must have a result");
    assert!(tool_msg["content"].as_str().unwrap().contains("Cancelled"));
}
