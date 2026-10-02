use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures::Stream;
use maukode_core::{
    ContextManager, RunnerConfig, Session, SessionRunner,
};
use maukode_protocol::{LLMEvent, Message, SessionId, SessionState};
use maukode_tools::create_default_registry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use crate::state::ServerState;

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    pub directory: Option<String>,
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub directory: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct SessionDetailResponse {
    pub id: SessionId,
    pub directory: String,
    pub state: SessionState,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn list_sessions(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let summaries = if query.all.unwrap_or(false) {
        state
            .store
            .list_all_sessions()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        let dir = query
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| state.default_dir.clone());
        state
            .store
            .list_sessions_by_directory(&dir)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    Ok(Json(summaries))
}

pub async fn create_session(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dir = payload
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());

    let session = Session::new(&dir);
    let meta = session.metadata();

    let config = state.config.read().await;
    state
        .store
        .save_session(&session, &config.active_provider, &config.active_model)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state.register_new_session(session).await;

    Ok(Json(meta))
}

pub async fn get_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let session = session_arc.read().await;
    let detail = SessionDetailResponse {
        id: session.id,
        directory: session.directory.to_string_lossy().to_string(),
        state: session.state.clone(),
        messages: session.messages.clone(),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
    };

    Ok(Json(detail))
}

pub async fn submit_prompt(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<PromptRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    {
        let session = session_arc.read().await;
        if matches!(session.state, SessionState::Running) {
            return Err((
                StatusCode::CONFLICT,
                "Agent is already executing a turn for this session".to_string(),
            ));
        }
    }

    let config_guard = state.config.read().await;
    let llm_client = config_guard
        .create_active_llm_client()
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let runner_config = RunnerConfig {
        model: config_guard.active_model.clone(),
        max_steps: 25,
        max_tokens: Some(4096),
        temperature: None,
    };
    let active_provider = config_guard.active_provider.clone();
    let active_model = config_guard.active_model.clone();

    let session_dir = session_arc.read().await.directory.clone();
    let tools = if config_guard.security.jail_path_traversal {
        let jail = maukode_tools::PathJail::new(&session_dir)
            .with_allowed_paths(config_guard.security.allowed_external_paths.clone());
        Arc::new(maukode_tools::create_default_registry_with_jail(&session_dir, jail))
    } else {
        Arc::new(create_default_registry(&session_dir))
    };
    drop(config_guard);
    let context = Arc::new(ContextManager::new(&session_dir));

    let runner = SessionRunner::new(
        Arc::clone(&session_arc),
        llm_client,
        tools,
        context,
        runner_config,
    ).with_permission_manager(Arc::clone(&state.permission_manager));

    let event_broadcast_tx = state.get_event_sender(id).await;
    let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<LLMEvent>();
    let store = Arc::clone(&state.store);
    let session_for_persist = Arc::clone(&session_arc);

    // Spawn execution loop in background
    tokio::spawn(async move {
        let run_handle = tokio::spawn(async move {
            runner.run(Some(payload.prompt), llm_tx).await
        });

        // Forward stream events to broadcast subscribers
        while let Some(event) = llm_rx.recv().await {
            let _ = event_broadcast_tx.send(event);
        }

        let _ = run_handle.await;

        // Persist final session state to SQLite
        let guard = session_for_persist.read().await;
        let _ = store.save_session(&guard, &active_provider, &active_model).await;
    });

    Ok(Json(json!({
        "status": "started",
        "session_id": id,
    })))
}

pub async fn stream_session_events(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let broadcaster = state.get_event_sender(id).await;
    let rx = broadcaster.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(llm_event) => {
            let json_str = serde_json::to_string(&llm_event).unwrap_or_default();
            Some(Ok(Event::default().data(json_str)))
        }
        Err(_) => None,
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Debug, Deserialize)]
pub struct RespondPermissionRequest {
    pub decision: maukode_protocol::PermissionDecision,
}

pub async fn respond_permission(
    State(state): State<Arc<ServerState>>,
    Path((_session_id, request_id)): Path<(SessionId, String)>,
    Json(payload): Json<RespondPermissionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let handled = state.permission_manager.respond(&request_id, payload.decision).await;
    if handled {
        Ok(Json(json!({ "status": "ok", "request_id": request_id })))
    } else {
        Err((StatusCode::NOT_FOUND, "Permission request not found or expired".to_string()))
    }
}
