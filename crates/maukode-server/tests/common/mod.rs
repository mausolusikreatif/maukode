//! Shared helpers for server integration tests.
#![allow(dead_code)]

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::Response,
    routing::post,
    Json, Router,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::net::TcpListener;

static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();

/// Point config/data at a throwaway directory for the whole test process, so tests never read
/// or write the developer's real MauKode database, config or worktrees.
pub fn isolate_home() {
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("MAUKODE_HOME", dir.path());
        dir
    });
}

/// One scripted provider response.
pub enum Step {
    /// SSE chunks (JSON strings); `[DONE]` is appended.
    Sse(Vec<String>),
    /// Fail the request with this HTTP status.
    Status(u16),
}

pub fn text_chunk(text: &str) -> String {
    serde_json::json!({"choices": [{"delta": {"content": text}}]}).to_string()
}

pub fn finish_chunk(reason: &str) -> String {
    serde_json::json!({"choices": [{"finish_reason": reason}]}).to_string()
}

pub fn tool_call_chunk(id: &str, name: &str, args: serde_json::Value) -> String {
    serde_json::json!({"choices": [{"delta": {"tool_calls": [{
        "index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}
    }]}}]})
    .to_string()
}

pub struct Gateway {
    /// Base URL to use as a provider's `base_url`.
    pub url: String,
    /// Every request body the agent sent, in order.
    pub requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

#[derive(Clone)]
struct GatewayState {
    script: Arc<Mutex<VecDeque<Step>>>,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

async fn completions(
    State(st): State<GatewayState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    st.requests.lock().unwrap().push(body);
    let step = st.script.lock().unwrap().pop_front();
    match step {
        Some(Step::Sse(chunks)) => {
            let mut out = String::new();
            for c in chunks {
                out.push_str(&format!("data: {c}\n\n"));
            }
            out.push_str("data: [DONE]\n\n");
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(out))
                .unwrap()
        }
        Some(Step::Status(code)) => Response::builder()
            .status(StatusCode::from_u16(code).unwrap())
            .body(Body::from("scripted failure"))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("gateway script exhausted"))
            .unwrap(),
    }
}

/// An OpenAI-compatible gateway that replays `script`, one step per request.
pub async fn start_gateway(script: Vec<Step>) -> Gateway {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let state = GatewayState {
        script: Arc::new(Mutex::new(script.into())),
        requests: Arc::clone(&requests),
    };
    let app = Router::new()
        .route("/v1/chat/completions", post(completions))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Gateway { url, requests }
}

/// Minimal SSE reader over a streaming reqwest response.
pub struct SseReader {
    resp: reqwest::Response,
    buf: String,
}

impl SseReader {
    pub fn new(resp: reqwest::Response) -> Self {
        Self {
            resp,
            buf: String::new(),
        }
    }

    /// Next `(id, data-as-json)`; skips keep-alive comments. `None` on timeout or stream end.
    pub async fn next(&mut self) -> Option<(Option<u64>, serde_json::Value)> {
        loop {
            if let Some(pos) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..pos + 2).collect();
                let mut id = None;
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("id:") {
                        id = v.trim().parse().ok();
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim_start());
                    }
                }
                if data.is_empty() {
                    continue;
                }
                return Some((id, serde_json::from_str(&data).expect("event json")));
            }
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(15), self.resp.chunk())
                .await
                .ok()?
                .ok()??;
            self.buf.push_str(&String::from_utf8_lossy(&chunk));
        }
    }

    /// Read until an event with `type == kind`; returns everything read, ending with that event.
    pub async fn until(&mut self, kind: &str) -> Vec<serde_json::Value> {
        let mut seen = Vec::new();
        while let Some((_, ev)) = self.next().await {
            let done = ev["type"] == kind;
            seen.push(ev);
            if done {
                return seen;
            }
        }
        panic!(
            "stream ended/timed out before '{kind}'; saw {:?}",
            seen.iter().map(|e| e["type"].clone()).collect::<Vec<_>>()
        );
    }
}
