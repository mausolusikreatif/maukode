use super::events::{resume_point, sse_response, EventsQuery};
use crate::state::ServerState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
    Json,
};
use futures::Stream;
use maukode_core::worktree::{self, WorktreeError, WorktreeInfo};
use maukode_core::{ContextManager, RunnerConfig, Session, SessionRunner};
use maukode_protocol::{GlobalEvent, LLMEvent, Message, SessionId, SessionState, StopReason};
use maukode_tools::create_default_registry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    pub directory: Option<String>,
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CreateSessionRequest {
    pub directory: Option<String>,
    pub title: Option<String>,
    /// Pin this session to a provider/model instead of the server defaults.
    pub provider: Option<String>,
    pub model: Option<String>,
    /// `"worktree"` runs the agent in its own git worktree/branch so parallel agents on the same
    /// repository cannot overwrite each other. Omit for the shared checkout.
    pub isolation: Option<Isolation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    Worktree,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CreateSessionResponse {
    #[serde(flatten)]
    pub meta: maukode_protocol::SessionMetadata,
    pub provider: String,
    pub model: String,
    pub worktree: Option<WorktreeInfo>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SessionListItem {
    #[serde(flatten)]
    pub summary: maukode_core::SessionSummary,
    pub running: bool,
    pub pending_permission_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct SessionDetailQuery {
    /// Return only the newest N messages (see `messages_start`); omit for the full history.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    /// Exclusive upper index; defaults to the end. Pass the previous page's `start`.
    pub before: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MessagesPage {
    pub session_id: SessionId,
    pub total: usize,
    /// Index of `messages[0]` in the full history; pass it as `before` for the next page.
    pub start: usize,
    pub end: usize,
    pub has_more: bool,
    pub messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PermissionModeResponse {
    pub mode: maukode_core::PermissionMode,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AuditResponse {
    pub session_id: SessionId,
    pub summary: String,
    pub report: maukode_core::store::sqlite::AuditReport,
}

const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 500;

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateSessionRequest {
    /// New title; null or blank restores the automatic title.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PromptRequest {
    pub prompt: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SessionDetailResponse {
    pub id: SessionId,
    pub title: Option<String>,
    /// True while an agent turn is executing.
    pub running: bool,
    /// Sequence number of the newest event; pass to `/events?since=` to resume without gaps.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub last_event_seq: u64,
    pub pending_permissions: Vec<maukode_protocol::PermissionRequest>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<WorktreeInfo>,
    /// Total messages in the session; `messages` may be a suffix when `limit` was given.
    pub message_total: usize,
    /// Index in the full history of `messages[0]`.
    pub messages_start: usize,
    pub directory: String,
    pub state: SessionState,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub updated_at: String,
}

/// Provider/model a session runs with: its pinned pair if the provider still exists, otherwise
/// the server defaults.
async fn effective_model(state: &ServerState, id: SessionId) -> (String, String) {
    let config = state.config.read().await;
    if let Ok(Some((provider, model))) = state.store.session_model(id).await {
        if config.get_provider(&provider).is_some() {
            return (provider, model);
        }
    }
    (config.active_provider.clone(), config.active_model.clone())
}

async fn load_session(
    state: &ServerState,
    id: SessionId,
) -> Result<Arc<tokio::sync::RwLock<Session>>, (StatusCode, String)> {
    state
        .get_or_load_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))
}

/// Checkpoint manager for the workspace this session actually runs in.
async fn checkpoints_for(
    state: &ServerState,
    id: SessionId,
) -> Result<Arc<maukode_core::CheckpointManager>, (StatusCode, String)> {
    let session = load_session(state, id).await?;
    let dir = session.read().await.directory.clone();
    state
        .checkpoint_manager_for(&dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn list_sessions(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Read before the listing so a client resuming from this seq cannot miss an event.
    let event_seq = state.global_events.last_seq();
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

    let mut items = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let id = summary.id;
        items.push(SessionListItem {
            summary,
            running: state.is_running(id).await,
            pending_permission_count: state.pending_permission_count(id).await,
        });
    }

    Ok(([("x-event-seq", event_seq.to_string())], Json(items)))
}

pub async fn create_session(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dir = payload
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());

    let (provider, model) = {
        let config = state.config.read().await;
        let provider = payload
            .provider
            .unwrap_or_else(|| config.active_provider.clone());
        let provider_cfg = config.get_provider(&provider).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("Provider '{provider}' not found"),
            )
        })?;
        let model = payload.model.unwrap_or_else(|| {
            if provider == config.active_provider {
                config.active_model.clone()
            } else {
                provider_cfg
                    .default_model
                    .clone()
                    .unwrap_or_else(|| config.active_model.clone())
            }
        });
        (provider, model)
    };

    let mut session = Session::new(&dir);
    let mut worktree_info = None;
    if payload.isolation == Some(Isolation::Worktree) {
        let info = worktree::create(&dir, session.id)
            .await
            .map_err(|e| match e {
                WorktreeError::Other(_) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
                _ => (StatusCode::BAD_REQUEST, e.to_string()),
            })?;
        session.directory = info.path.clone();
        worktree_info = Some(info);
    }
    let mut meta = session.metadata();

    let persisted = async {
        state
            .store
            .save_session(&session, &provider, &model)
            .await?;
        if let Some(info) = &worktree_info {
            state.store.set_worktree(session.id, info).await?;
        }
        if let Some(title) = payload
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            state
                .store
                .set_session_title(session.id, Some(title))
                .await?;
            meta.title = Some(title.to_string());
        }
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = persisted {
        // Don't leave an orphaned worktree behind a session that was never stored.
        if let Some(info) = &worktree_info {
            let _ = worktree::remove(info, true).await;
        }
        let _ = state.store.delete_session(session.id).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    let id = session.id;
    state.register_new_session(session).await;
    state.emit(GlobalEvent::SessionCreated { session_id: id });

    Ok(Json(CreateSessionResponse {
        meta,
        provider,
        model,
        worktree: worktree_info,
    }))
}

pub async fn get_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query): Query<SessionDetailQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = load_session(&state, id).await?;

    let title = state.store.session_title(id).await.ok().flatten();
    let model = state.store.session_model(id).await.ok().flatten();
    let worktree = state.store.get_worktree(id).await.ok().flatten();
    let running = state.is_running(id).await;
    let last_event_seq = state.channel(id).await.last_seq();
    let pending_permissions = state.pending_permissions_for(id).await;
    let session = session_arc.read().await;
    let message_total = session.messages.len();
    let messages_start = query
        .limit
        .map_or(0, |limit| message_total.saturating_sub(limit.min(MAX_PAGE)));
    let detail = SessionDetailResponse {
        id: session.id,
        title,
        running,
        last_event_seq,
        pending_permissions,
        provider: model.as_ref().map(|(p, _)| p.clone()),
        model: model.map(|(_, m)| m),
        worktree,
        message_total,
        messages_start,
        directory: session.directory.to_string_lossy().to_string(),
        state: session.state.clone(),
        messages: session.messages[messages_start..].to_vec(),
        created_at: session.created_at.to_rfc3339(),
        updated_at: session.updated_at.to_rfc3339(),
    };

    Ok(Json(detail))
}

/// Page backwards through history: first call without `before`, then pass the returned `start`.
pub async fn get_session_messages(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query): Query<MessagesQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = load_session(&state, id).await?;
    let session = session_arc.read().await;
    let total = session.messages.len();
    let end = query.before.unwrap_or(total).min(total);
    let limit = query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let start = end.saturating_sub(limit);
    Ok(Json(MessagesPage {
        session_id: id,
        total,
        start,
        end,
        has_more: start > 0,
        messages: session.messages[start..end].to_vec(),
    }))
}

pub async fn submit_prompt(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<PromptRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let session_arc = load_session(&state, id).await?;

    let cancellation_token = state.try_register_active_turn(id).await.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            "Agent is already executing a turn for this session".to_string(),
        )
    })?;

    // Past this point every early return must release the turn slot.
    if let Err(err) = start_turn(
        Arc::clone(&state),
        id,
        session_arc,
        payload.prompt,
        cancellation_token,
    )
    .await
    {
        state.clear_active_turn(id).await;
        return Err(err);
    }

    Ok(Json(json!({
        "status": "started",
        "session_id": id,
    })))
}

async fn persist_session(
    state: &ServerState,
    id: SessionId,
    session: &Arc<tokio::sync::RwLock<Session>>,
    provider: &str,
    model: &str,
) {
    // Holding the map lock while saving orders this against `delete_session`, so a deleted
    // session is never written back.
    let sessions = state.sessions.read().await;
    if !sessions.contains_key(&id) {
        return;
    }
    let guard = session.read().await;
    if let Err(error) = state.store.save_session(&guard, provider, model).await {
        tracing::warn!(%error, session = %id, "Failed to persist session");
    }
}

async fn start_turn(
    state: Arc<ServerState>,
    id: SessionId,
    session_arc: Arc<tokio::sync::RwLock<Session>>,
    prompt: String,
    cancellation_token: tokio_util::sync::CancellationToken,
) -> Result<(), (StatusCode, String)> {
    let (provider_id, model) = effective_model(&state, id).await;
    let session_dir = session_arc.read().await.directory.clone();

    let config_guard = state.config.read().await;
    let llm_client = config_guard
        .create_llm_client_for(&provider_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let runner_config = RunnerConfig {
        model: model.clone(),
        max_steps: config_guard.agent.max_steps,
        max_tokens: Some(config_guard.agent.max_tokens),
        temperature: None,
    };

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

    let mut runner = SessionRunner::new(
        Arc::clone(&session_arc),
        llm_client,
        tools,
        context,
        runner_config,
    )
    .with_permission_manager(state.permission_manager_for(id).await)
    .with_permission_store(Arc::clone(&state.store));
    match state.checkpoint_manager_for(&session_dir).await {
        Ok(cm) => runner = runner.with_checkpoint_manager(cm),
        Err(error) => tracing::warn!(%error, "Checkpoints unavailable for this turn"),
    }

    let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<LLMEvent>();

    state.emit(GlobalEvent::TurnStarted { session_id: id });

    tokio::spawn(async move {
        let run_handle = tokio::spawn(async move {
            runner
                .run_with_cancellation(Some(prompt), llm_tx, Some(cancellation_token))
                .await
        });

        // Forward stream events; checkpoint progress to SQLite at most every 2s so a crash
        // costs seconds of work rather than the whole turn.
        let mut last_save = std::time::Instant::now();
        let mut stopped: Option<(StopReason, Option<String>)> = None;
        while let Some(event) = llm_rx.recv().await {
            if let LLMEvent::TurnStopped { reason, message } = &event {
                stopped = Some((*reason, message.clone()));
            }
            let save_now = matches!(event, LLMEvent::ToolResult(_))
                && last_save.elapsed() >= std::time::Duration::from_secs(2);
            state.publish_event(id, event).await;
            if save_now {
                persist_session(&state, id, &session_arc, &provider_id, &model).await;
                last_save = std::time::Instant::now();
            }
        }

        let join = run_handle.await;
        // The runner always closes a turn with `TurnStopped`; if it did not (the task panicked),
        // synthesize it so clients are never left waiting on a dead turn.
        let (reason, message) = match stopped {
            Some(done) => done,
            None => {
                let message = match join {
                    Err(e) => format!("Agent task crashed: {e}"),
                    Ok(_) => "Agent turn ended unexpectedly".to_string(),
                };
                session_arc.write().await.state = SessionState::Failed(message.clone());
                let error = LLMEvent::Error {
                    message: message.clone(),
                };
                state.publish_event(id, error).await;
                let stop = LLMEvent::TurnStopped {
                    reason: StopReason::Error,
                    message: Some(message.clone()),
                };
                state.publish_event(id, stop).await;
                (StopReason::Error, Some(message))
            }
        };

        // Free the turn slot first so the next prompt can proceed, then persist final state.
        state.clear_active_turn(id).await;
        state.clear_pending_permissions(id).await;
        persist_session(&state, id, &session_arc, &provider_id, &model).await;
        state.emit(GlobalEvent::TurnFinished {
            session_id: id,
            reason,
            message,
        });
    });

    Ok(())
}

pub async fn stream_session_events(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let since = resume_point(&query, &headers);
    sse_response(state.channel(id).await, since)
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetPermissionModeRequest {
    pub mode: maukode_core::PermissionMode,
    /// With a session id only that session changes; without one the default for new sessions
    /// and every existing session is updated.
    pub session_id: Option<maukode_protocol::SessionId>,
}

#[derive(Debug, Deserialize)]
pub struct GetPermissionModeQuery {
    pub session_id: Option<maukode_protocol::SessionId>,
}

pub async fn get_permission_mode(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<GetPermissionModeQuery>,
) -> impl IntoResponse {
    let mode = match query.session_id {
        Some(id) => state.permission_manager_for(id).await.mode(),
        None => state.permission_manager.mode(),
    };
    Json(PermissionModeResponse { mode })
}

pub async fn set_permission_mode(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<SetPermissionModeRequest>,
) -> impl IntoResponse {
    let previous_mode = match payload.session_id {
        Some(id) => {
            let manager = state.permission_manager_for(id).await;
            let previous = manager.mode();
            manager.set_mode(payload.mode);
            previous
        }
        None => {
            let previous = state.permission_manager.mode();
            state.permission_manager.set_mode(payload.mode);
            for manager in state.session_permissions.read().await.values() {
                manager.set_mode(payload.mode);
            }
            previous
        }
    };
    if let Some(session_id) = payload.session_id {
        let session = state.sessions.read().await.get(&session_id).cloned();
        let turn_id = match session {
            Some(session) => session.read().await.messages.len() as i64,
            None => 0,
        };
        let record = maukode_core::store::sqlite::PermissionModeAuditRecord {
            session_id,
            turn_id,
            previous_mode,
            new_mode: payload.mode,
            source: "server_api".to_string(),
            created_at: chrono::Utc::now(),
        };
        if let Err(error) = state.store.record_permission_mode_change(&record).await {
            tracing::warn!(%error, "Failed to persist permission mode audit record");
        }
    }
    Json(json!({ "status": "ok", "mode": payload.mode }))
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RespondPermissionRequest {
    pub decision: maukode_protocol::PermissionDecision,
}

pub async fn respond_permission(
    State(state): State<Arc<ServerState>>,
    Path((session_id, request_id)): Path<(SessionId, String)>,
    Json(payload): Json<RespondPermissionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let handled = state
        .permission_manager
        .respond(&request_id, payload.decision)
        .await;
    state
        .resolve_pending_permission(session_id, &request_id)
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
    let diff = checkpoints_for(&state, id)
        .await?
        .get_diff(&id.to_string(), None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(diff))
}

#[derive(Debug, Deserialize, Default)]
pub struct DiffTurnQuery {
    /// Compare against the checkpoint taken before this turn instead of the session baseline.
    pub turn: Option<usize>,
}

pub async fn get_session_diff_files(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query): Query<DiffTurnQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let files = checkpoints_for(&state, id)
        .await?
        .get_diff_files(&id.to_string(), query.turn)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(maukode_protocol::FileDiffListResponse {
        session_id: id.to_string(),
        files,
    }))
}

#[derive(Debug, Deserialize)]
pub struct FileDiffQuery {
    pub path: String,
    pub turn: Option<usize>,
}

pub async fn get_session_file_diff(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query): Query<FileDiffQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let diff = checkpoints_for(&state, id)
        .await?
        .get_file_diff(&id.to_string(), &query.path, query.turn)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(diff))
}

#[derive(Debug, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UndoRequestPayload {
    pub turns: Option<usize>,
}

pub async fn undo_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<UndoRequestPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if state.is_running(id).await {
        return Err((
            StatusCode::CONFLICT,
            "Cannot undo while the agent is running; cancel the turn first".to_string(),
        ));
    }
    let undo_res = checkpoints_for(&state, id)
        .await?
        .undo(&id.to_string(), payload.turns)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(undo_res))
}

pub async fn list_session_checkpoints(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let checkpoints = checkpoints_for(&state, id)
        .await?
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
    let (provider_id, model) = effective_model(&state, id).await;
    let llm_client = state
        .config
        .read()
        .await
        .create_llm_client_for(&provider_id)
        .ok();

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
    let _ = state
        .store
        .save_session(&session, &provider_id, &model)
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

#[derive(Debug, Deserialize, Default)]
pub struct DeleteSessionQuery {
    /// Remove the session's worktree checkout (default true).
    pub cleanup_worktree: Option<bool>,
    /// Also delete its branch, discarding unmerged commits (default false).
    pub delete_branch: Option<bool>,
}

pub async fn delete_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Query(query): Query<DeleteSessionQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state.cancel_active_turn(id).await;
    let worktree_info = state.store.get_worktree(id).await.ok().flatten();
    // Drop in-memory state first: this waits out any in-flight save, and later saves from the
    // cancelled turn see the session is gone and skip.
    state.forget_session(id).await;
    let existed = state
        .store
        .delete_session(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !existed {
        return Err((StatusCode::NOT_FOUND, "Session not found".to_string()));
    }
    if let (Some(info), true) = (worktree_info, query.cleanup_worktree.unwrap_or(true)) {
        if let Err(error) = worktree::remove(&info, query.delete_branch.unwrap_or(false)).await {
            tracing::warn!(%error, "Failed to remove worktree");
        }
    }
    state.emit(GlobalEvent::SessionDeleted { session_id: id });
    Ok(Json(json!({ "status": "deleted", "session_id": id })))
}

pub async fn get_session_audit(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let summary = state
        .store
        .audit_summary(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let report = state
        .store
        .audit_report(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(AuditResponse {
        session_id: id,
        summary,
        report,
    }))
}

pub async fn update_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<UpdateSessionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let title = payload
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let found = state
        .store
        .set_session_title(id, title)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !found {
        return Err((StatusCode::NOT_FOUND, "Session not found".to_string()));
    }
    let title = state.store.session_title(id).await.ok().flatten();
    Ok(Json(
        json!({ "status": "ok", "session_id": id, "title": title }),
    ))
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetSessionModelRequest {
    /// Defaults to the session's current provider.
    pub provider_id: Option<String>,
    pub model: String,
}

/// Pin this session to a provider/model without touching other sessions or the defaults.
pub async fn set_session_model(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    Json(payload): Json<SetSessionModelRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    load_session(&state, id).await?;
    let provider = match payload.provider_id {
        Some(p) => p,
        None => effective_model(&state, id).await.0,
    };
    if state.config.read().await.get_provider(&provider).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Provider '{provider}' not found"),
        ));
    }
    state
        .store
        .set_session_model(id, &provider, &payload.model)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state.emit(GlobalEvent::SessionUpdated { session_id: id });
    Ok(Json(
        json!({ "status": "ok", "provider": provider, "model": payload.model }),
    ))
}

#[derive(Debug, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WorktreeActionRequest {
    pub message: Option<String>,
}

async fn worktree_of(
    state: &ServerState,
    id: SessionId,
) -> Result<WorktreeInfo, (StatusCode, String)> {
    load_session(state, id).await?;
    state
        .store
        .get_worktree(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Session does not use a worktree".to_string(),
            )
        })
}

pub async fn get_worktree(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let info = worktree_of(&state, id).await?;
    let status = worktree::status(&info)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(status))
}

fn reject_while_running(running: bool) -> Result<(), (StatusCode, String)> {
    if running {
        Err((
            StatusCode::CONFLICT,
            "The agent is running; wait for it to finish or cancel the turn".to_string(),
        ))
    } else {
        Ok(())
    }
}

/// Commit everything the agent changed onto the session's branch.
pub async fn commit_worktree(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    body: Option<Json<WorktreeActionRequest>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let info = worktree_of(&state, id).await?;
    reject_while_running(state.is_running(id).await)?;
    let message = body
        .and_then(|Json(b)| b.message)
        .unwrap_or_else(|| "maukode: agent changes".to_string());
    let commit = worktree::commit_all(&info, &message)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "commit": commit })))
}

/// Merge the session's branch into the project's current branch (`--no-ff`). Conflicts are
/// reported and rolled back; a dirty project checkout is refused with 409.
pub async fn merge_worktree(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<SessionId>,
    body: Option<Json<WorktreeActionRequest>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let info = worktree_of(&state, id).await?;
    reject_while_running(state.is_running(id).await)?;
    let message = body
        .and_then(|Json(b)| b.message)
        .unwrap_or_else(|| format!("Merge {}", info.branch));
    let outcome = worktree::merge(&info, &message)
        .await
        .map_err(|e| match e {
            WorktreeError::DirtyProject => (StatusCode::CONFLICT, e.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    state.emit(GlobalEvent::SessionUpdated { session_id: id });
    Ok(Json(outcome))
}
