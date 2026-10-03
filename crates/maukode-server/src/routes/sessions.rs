use crate::state::ServerState;
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
use maukode_core::{ContextManager, RunnerConfig, Session, SessionRunner};
use maukode_protocol::{LLMEvent, Message, SessionId, SessionState};
use maukode_tools::create_default_registry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

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
    let mut tool_registry = if config_guard.security.jail_path_traversal {
        let jail = maukode_tools::PathJail::new(&session_dir)
            .with_allowed_paths(config_guard.security.allowed_external_paths.clone());
        maukode_tools::create_default_registry_with_jail(&session_dir, jail)
    } else {
        create_default_registry(&session_dir)
    };
    config_guard.configure_web_search_in_registry(&mut tool_registry);
    state
        .mcp_manager
        .register_tools_into_registry(&mut tool_registry, &session_dir, &config_guard)
        .await;
    drop(config_guard);
    let context = Arc::new(ContextManager::new(&session_dir));
    tool_registry.register(Arc::new(maukode_core::CodebaseMapTool::new(Arc::clone(
        context.repo_map(),
    ))));
    let tools = Arc::new(tool_registry);

    let runner = SessionRunner::new(
        Arc::clone(&session_arc),
        llm_client,
        tools,
        context,
        runner_config,
    )
    .with_permission_manager(Arc::clone(&state.permission_manager))
    .with_checkpoint_manager(Arc::clone(&state.checkpoint_manager));

    let event_broadcast_tx = state.get_event_sender(id).await;
    let cancellation_token = state.register_active_turn(id).await;
    let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<LLMEvent>();
    let store = Arc::clone(&state.store);
    let session_for_persist = Arc::clone(&session_arc);
    let active_turns = Arc::clone(&state.active_turns);
    let clear_session_id = id;

    // Spawn execution loop in background
    tokio::spawn(async move {
        let run_handle = tokio::spawn(async move {
            runner
                .run_with_cancellation(Some(payload.prompt), llm_tx, Some(cancellation_token))
                .await
        });

        // Forward stream events to broadcast subscribers
        while let Some(event) = llm_rx.recv().await {
            let _ = event_broadcast_tx.send(event);
        }

        let _ = run_handle.await;

        // Clear the active turn marker so future prompts can proceed.
        active_turns.write().await.remove(&clear_session_id);

        // Persist final session state to SQLite
        let guard = session_for_persist.read().await;
        let _ = store
            .save_session(&guard, &active_provider, &active_model)
            .await;
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

pub async fn cancel_prompt(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let cancelled = state.cancel_active_turn(id).await;
    let status = if cancelled {
        "cancelled"
    } else {
        "no_active_turn"
    };
    Ok(Json(json!({
        "status": status,
        "session_id": id,
    })))
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
    let handled = state
        .permission_manager
        .respond(&request_id, payload.decision)
        .await;
    if handled {
        Ok(Json(json!({ "status": "ok", "request_id": request_id })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            "Permission request not found or expired".to_string(),
        ))
    }
}

pub async fn get_session_diff(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let diff = state
        .checkpoint_manager
        .get_diff(&id.to_string(), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(diff))
}

#[derive(Debug, Deserialize, Default)]
pub struct UndoRequestPayload {
    pub turns: Option<usize>,
}

pub async fn undo_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<UndoRequestPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let undo_res = state
        .checkpoint_manager
        .undo(&id.to_string(), payload.turns)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(undo_res))
}

pub async fn list_session_checkpoints(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let checkpoints = state
        .checkpoint_manager
        .list_checkpoints(&id.to_string())
        .await;
    Ok(Json(checkpoints))
}

#[derive(Debug, Deserialize, Default)]
pub struct CodebaseMapQuery {
    pub subpath: Option<String>,
    pub query: Option<String>,
    pub max_tokens: Option<usize>,
}

pub async fn get_codebase_map(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query_params): Query<CodebaseMapQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let session_dir = session_arc.read().await.directory.clone();
    let repo_map = maukode_core::RepoMapGenerator::new(&session_dir);
    let tokens = query_params.max_tokens.unwrap_or(1000);
    let map = repo_map
        .generate_map(
            tokens,
            query_params.subpath.as_deref(),
            query_params.query.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(maukode_protocol::CodebaseMapResponse {
        session_id: id.to_string(),
        map,
    }))
}

pub async fn init_project(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<maukode_protocol::InitProjectRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let session_dir = session_arc.read().await.directory.clone();
    let res = maukode_core::init_project(&session_dir, payload.force)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(maukode_protocol::InitProjectResponse {
        session_id: id.to_string(),
        path: res.path.display().to_string(),
        created: res.created,
        overwritten: res.overwritten,
        detected_type: res.detected_type,
        message: res.message,
        content: res.content,
    }))
}

pub async fn list_mcp_servers(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let session_dir = session_arc.read().await.directory.clone();
    let config_guard = state.config.read().await;
    let servers = state
        .mcp_manager
        .list_server_statuses(&session_dir, &config_guard)
        .await;

    Ok(Json(maukode_protocol::McpListResponse { servers }))
}

pub async fn compact_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<maukode_protocol::CompactRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let mut session = session_arc.write().await;
    let config_guard = state.config.read().await;
    let model = config_guard.active_model.clone();
    let llm_res = config_guard.create_active_llm_client();
    let llm_client = llm_res.ok();
    drop(config_guard);

    let result = maukode_core::ConversationCompactor::compact(
        &mut session,
        llm_client.as_deref(),
        &model,
        payload.focus.as_deref(),
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Archive the uncompressed original messages in SQLite audit store
    let _ = state
        .store
        .archive_session_messages(id, &result.archived_messages)
        .await;

    // Save the updated compacted session
    let config_guard = state.config.read().await;
    let _ = state
        .store
        .save_session(
            &session,
            &config_guard.active_provider,
            &config_guard.active_model,
        )
        .await;

    Ok(Json(maukode_protocol::CompactResponse {
        session_id: id.to_string(),
        original_messages: result.original_message_count,
        compacted_messages: result.compacted_message_count,
        original_tokens: result.original_tokens,
        compacted_tokens: result.compacted_tokens,
        summary: result.summary,
    }))
}
