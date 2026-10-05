//! Sign in with ChatGPT, driven over HTTP so a UI can run the flow.
//!
//! The protocol itself lives in `maukode_core::ChatGptAuth` (OAuth authorization code + PKCE with
//! a loopback callback). The daemon hosts that callback listener, so the browser used to approve
//! the sign-in must run on the **same machine** as the daemon. Tokens never leave this process:
//! responses carry only the account email and client id.

use crate::state::{LoginProgress, LoginSlot, ServerState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use maukode_core::ChatGptAuth;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatGptAccount {
    pub client_id: String,
    pub email: String,
    pub active: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatGptStatus {
    /// Email of the active account, if signed in.
    pub email: Option<String>,
    pub accounts: Vec<ChatGptAccount>,
}

#[derive(Debug, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatGptLoginRequest {
    /// Register an additional ChatGPT account instead of re-using the active one.
    #[serde(default)]
    pub new_account: bool,
    /// Ask the daemon to open the system browser (use when the UI cannot, e.g. a desktop webview).
    #[serde(default)]
    pub open_browser: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatGptLoginStarted {
    pub attempt_id: String,
    /// Authorization URL to open in a browser on the machine running the daemon.
    pub url: String,
    pub browser_opened: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatGptLoginStatus {
    pub attempt_id: String,
    #[serde(flatten)]
    pub progress: LoginProgress,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChatGptUseAccountRequest {
    pub client_id: String,
}

fn auth() -> Result<ChatGptAuth, (StatusCode, String)> {
    ChatGptAuth::new().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn status() -> Result<impl IntoResponse, (StatusCode, String)> {
    let auth = auth()?;
    let accounts = auth
        .accounts()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_iter()
        .map(|(client_id, email, active)| ChatGptAccount {
            client_id,
            email,
            active,
        })
        .collect();
    let email = auth
        .status()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ChatGptStatus { email, accounts }))
}

pub async fn begin_login(
    State(state): State<Arc<ServerState>>,
    body: Option<Json<ChatGptLoginRequest>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let auth = auth()?;
    let attempt = auth
        .begin_login(req.new_account)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let url = attempt.url.clone();
    let attempt_id = uuid::Uuid::new_v4().to_string();

    let progress = Arc::new(tokio::sync::Mutex::new(LoginProgress::Pending));
    let shared = Arc::clone(&progress);
    let task = tokio::spawn(async move {
        let result = auth.finish_login(attempt).await;
        *shared.lock().await = match result {
            Ok(email) => LoginProgress::Succeeded { email },
            Err(e) => LoginProgress::Failed {
                error: e.to_string(),
            },
        };
    });

    // One sign-in at a time: starting another cancels (and closes the listener of) the old one.
    if let Some(old) = state.login.lock().await.replace(LoginSlot {
        id: attempt_id.clone(),
        progress,
        task,
    }) {
        old.task.abort();
    }

    let browser_opened = req.open_browser && maukode_core::browser::open_url(&url);
    Ok(Json(ChatGptLoginStarted {
        attempt_id,
        url,
        browser_opened,
    }))
}

pub async fn login_status(
    State(state): State<Arc<ServerState>>,
    Path(attempt_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let slot = state.login.lock().await;
    match slot.as_ref().filter(|s| s.id == attempt_id) {
        Some(s) => Ok(Json(ChatGptLoginStatus {
            attempt_id,
            progress: s.progress.lock().await.clone(),
        })),
        None => Err((StatusCode::NOT_FOUND, "Unknown sign-in attempt".to_string())),
    }
}

pub async fn cancel_login(
    State(state): State<Arc<ServerState>>,
    Path(attempt_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut slot = state.login.lock().await;
    if slot.as_ref().is_some_and(|s| s.id == attempt_id) {
        if let Some(old) = slot.take() {
            old.task.abort();
        }
        Ok(Json(serde_json::json!({ "status": "cancelled" })))
    } else {
        Err((StatusCode::NOT_FOUND, "Unknown sign-in attempt".to_string()))
    }
}

pub async fn use_account(
    Json(req): Json<ChatGptUseAccountRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    auth()?
        .use_account(&req.client_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

pub async fn logout() -> Result<impl IntoResponse, (StatusCode, String)> {
    auth()?
        .logout()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({ "status": "signed_out" })))
}
