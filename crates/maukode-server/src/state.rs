use anyhow::Result;
use maukode_core::CheckpointManager;
use maukode_core::{AppConfig, PermissionManager, Session, SqliteSessionStore};
use maukode_protocol::{GlobalEvent, LLMEvent, PermissionRequest, SessionId, SessionState};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;

pub struct PreviewProcess {
    pub child: tokio::process::Child,
    pub info: maukode_protocol::PreviewInfo,
}

/// Number of recent events kept per session so reconnecting clients can catch up.
const EVENT_LOG_CAPACITY: usize = 2000;

/// An event with a monotonically increasing sequence number (SSE `id`).
#[derive(Debug, Clone, Serialize)]
pub struct SeqEvent<T> {
    pub seq: u64,
    pub event: T,
}

struct ChannelInner<T> {
    last_seq: u64,
    log: VecDeque<SeqEvent<T>>,
}

/// Broadcast channel plus a bounded replay log.
pub struct EventChannel<T> {
    tx: broadcast::Sender<SeqEvent<T>>,
    inner: std::sync::Mutex<ChannelInner<T>>,
}

/// Per-session agent events.
pub type SessionChannel = EventChannel<LLMEvent>;

impl<T: Clone> EventChannel<T> {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(500);
        Self {
            tx,
            inner: std::sync::Mutex::new(ChannelInner {
                last_seq: 0,
                log: VecDeque::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ChannelInner<T>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Append to the log and broadcast; returns the assigned sequence number.
    pub fn publish(&self, event: T) -> u64 {
        let mut inner = self.lock();
        inner.last_seq += 1;
        let ev = SeqEvent {
            seq: inner.last_seq,
            event,
        };
        if inner.log.len() == EVENT_LOG_CAPACITY {
            inner.log.pop_front();
        }
        inner.log.push_back(ev.clone());
        // Sent under the lock so subscribers never see gaps or duplicates around `subscribe`.
        let _ = self.tx.send(ev);
        inner.last_seq
    }

    pub fn last_seq(&self) -> u64 {
        self.lock().last_seq
    }

    /// Logged events with `seq > after`.
    pub fn replay_after(&self, after: u64) -> Vec<SeqEvent<T>> {
        self.lock()
            .log
            .iter()
            .filter(|e| e.seq > after)
            .cloned()
            .collect()
    }

    /// Atomically snapshot the backlog (`seq > since`, if given) and subscribe to live events.
    /// Returns `(backlog, receiver, last_seq_seen)`; without `since` the backlog is empty.
    #[allow(clippy::type_complexity)]
    pub fn subscribe(
        &self,
        since: Option<u64>,
    ) -> (Vec<SeqEvent<T>>, broadcast::Receiver<SeqEvent<T>>, u64) {
        let inner = self.lock();
        let rx = self.tx.subscribe();
        let after = since.unwrap_or(inner.last_seq);
        let backlog = match since {
            Some(s) => inner.log.iter().filter(|e| e.seq > s).cloned().collect(),
            None => Vec::new(),
        };
        (backlog, rx, after)
    }
}

impl<T: Clone> Default for EventChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Where a ChatGPT sign-in attempt currently stands.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoginProgress {
    Pending,
    Succeeded { email: String },
    Failed { error: String },
}

/// The single in-flight ChatGPT sign-in (its task owns the loopback callback listener).
pub struct LoginSlot {
    pub id: String,
    pub progress: Arc<tokio::sync::Mutex<LoginProgress>>,
    pub task: tokio::task::JoinHandle<()>,
}

pub struct ServerState {
    pub login: tokio::sync::Mutex<Option<LoginSlot>>,
    pub config: Arc<RwLock<AppConfig>>,
    pub store: Arc<SqliteSessionStore>,
    pub sessions: Arc<RwLock<HashMap<SessionId, Arc<RwLock<Session>>>>>,
    pub channels: Arc<RwLock<HashMap<SessionId, Arc<SessionChannel>>>>,
    pub pending_permissions: Arc<RwLock<HashMap<SessionId, HashMap<String, PermissionRequest>>>>,
    pub session_permissions: Arc<RwLock<HashMap<SessionId, Arc<PermissionManager>>>>,
    pub active_turns: Arc<RwLock<HashMap<SessionId, CancellationToken>>>,
    /// Session-level queues for prompts waiting while an agent turn is active.
    pub queued_prompts: Arc<RwLock<HashMap<SessionId, VecDeque<String>>>>,
    /// Default manager; per-session managers are forked from it (see `permission_manager_for`).
    pub permission_manager: Arc<PermissionManager>,
    /// One shadow-git manager per workspace root (worktree sessions each have their own).
    pub checkpoint_managers: Arc<RwLock<HashMap<PathBuf, Arc<CheckpointManager>>>>,
    /// Cross-session activity feed for dashboards.
    pub global_events: Arc<EventChannel<GlobalEvent>>,
    pub mcp_manager: Arc<maukode_core::McpManager>,
    pub skill_manager: Arc<maukode_core::SkillManager>,
    pub plugin_manager: Arc<RwLock<maukode_core::PluginManager>>,
    pub auth_token: Option<String>,
    /// Reject requests whose `Host` is not a loopback name (DNS-rebinding guard). Only meaningful
    /// without a token on a loopback bind; `run_server` turns it off for other bind addresses.
    pub enforce_local_host: bool,
    pub default_dir: PathBuf,
    /// At most one daemon-owned preview process per session.
    pub previews: Arc<tokio::sync::Mutex<HashMap<SessionId, PreviewProcess>>>,
    /// Shared HTTP client used by the preview reverse proxy for connection pooling.
    pub proxy_client: reqwest::Client,
}

impl ServerState {
    pub async fn new(auth_token: Option<String>, default_dir: Option<PathBuf>) -> Result<Self> {
        let config = AppConfig::load();
        let store = SqliteSessionStore::new(SqliteSessionStore::default_db_path())?;
        let dir = default_dir
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let permission_manager = Arc::new(
            PermissionManager::new(config.security.permission_mode)
                .with_whitelist(config.security.bash_whitelist.clone()),
        );

        let mcp_manager = Arc::new(maukode_core::McpManager::new());
        let skill_manager = Arc::new(maukode_core::SkillManager::new());
        let plugin_manager = Arc::new(RwLock::new(maukode_core::PluginManager::new()));

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            store: Arc::new(store),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            login: tokio::sync::Mutex::new(None),
            channels: Arc::new(RwLock::new(HashMap::new())),
            pending_permissions: Arc::new(RwLock::new(HashMap::new())),
            session_permissions: Arc::new(RwLock::new(HashMap::new())),
            active_turns: Arc::new(RwLock::new(HashMap::new())),
            queued_prompts: Arc::new(RwLock::new(HashMap::new())),
            permission_manager,
            checkpoint_managers: Arc::new(RwLock::new(HashMap::new())),
            global_events: Arc::new(EventChannel::new()),
            mcp_manager,
            skill_manager,
            plugin_manager,
            enforce_local_host: auth_token.is_none(),
            auth_token,
            default_dir: dir,
            previews: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            proxy_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    pub async fn get_or_load_session(&self, id: SessionId) -> Result<Option<Arc<RwLock<Session>>>> {
        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.get(&id) {
            return Ok(Some(Arc::clone(s)));
        }

        if let Some(mut loaded) = self.store.load_session(id).await? {
            // A turn that was in flight when the process died can never resume; without this the
            // session would look busy forever.
            if matches!(
                loaded.state,
                SessionState::Running
                    | SessionState::WaitingForInput
                    | SessionState::WaitingForToolConfirmation
            ) && !self.active_turns.read().await.contains_key(&id)
            {
                loaded.state = SessionState::Interrupted;
            }
            let session_arc = Arc::new(RwLock::new(loaded));
            sessions.insert(id, Arc::clone(&session_arc));
            return Ok(Some(session_arc));
        }

        Ok(None)
    }

    pub async fn register_new_session(&self, session: Session) -> Arc<RwLock<Session>> {
        let id = session.id;
        let session_arc = Arc::new(RwLock::new(session));
        let mut sessions = self.sessions.write().await;
        sessions.insert(id, Arc::clone(&session_arc));
        session_arc
    }

    /// Atomically claim the session's turn slot; `None` if a turn is already running.
    pub async fn try_register_active_turn(&self, id: SessionId) -> Option<CancellationToken> {
        let mut turns = self.active_turns.write().await;
        if turns.contains_key(&id) {
            return None;
        }
        let token = CancellationToken::new();
        turns.insert(id, token.clone());
        Some(token)
    }

    pub async fn cancel_active_turn(&self, id: SessionId) -> bool {
        let mut turns = self.active_turns.write().await;
        if let Some(token) = turns.remove(&id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn clear_active_turn(&self, id: SessionId) {
        let mut turns = self.active_turns.write().await;
        turns.remove(&id);
    }

    pub async fn channel(&self, id: SessionId) -> Arc<SessionChannel> {
        let mut channels = self.channels.write().await;
        Arc::clone(
            channels
                .entry(id)
                .or_insert_with(|| Arc::new(SessionChannel::new())),
        )
    }

    /// Publish a runner event to subscribers, tracking pending permission prompts so that
    /// clients that (re)connect mid-turn can still answer them.
    pub async fn publish_event(&self, id: SessionId, event: LLMEvent) {
        if let LLMEvent::PermissionRequest(req) = &event {
            self.pending_permissions
                .write()
                .await
                .entry(id)
                .or_default()
                .insert(req.request_id.clone(), req.clone());
            self.emit(GlobalEvent::PermissionRequested {
                session_id: id,
                request: req.clone(),
            });
        }
        self.channel(id).await.publish(event);
    }

    /// Publish to the cross-session feed.
    pub fn emit(&self, event: GlobalEvent) {
        self.global_events.publish(event);
    }

    pub async fn resolve_pending_permission(&self, id: SessionId, request_id: &str) {
        let removed = {
            let mut pending = self.pending_permissions.write().await;
            let removed = pending
                .get_mut(&id)
                .is_some_and(|map| map.remove(request_id).is_some());
            if pending.get(&id).is_some_and(|m| m.is_empty()) {
                pending.remove(&id);
            }
            removed
        };
        if removed {
            self.emit(GlobalEvent::PermissionResolved {
                session_id: id,
                request_id: request_id.to_string(),
            });
        }
    }

    /// Drop prompts nobody answered (turn ended, timed out or was cancelled).
    pub async fn clear_pending_permissions(&self, id: SessionId) {
        let removed = self.pending_permissions.write().await.remove(&id);
        for request_id in removed.into_iter().flat_map(|m| m.into_keys()) {
            self.emit(GlobalEvent::PermissionResolved {
                session_id: id,
                request_id,
            });
        }
    }

    pub async fn pending_permission_count(&self, id: SessionId) -> usize {
        self.pending_permissions
            .read()
            .await
            .get(&id)
            .map_or(0, |m| m.len())
    }

    pub async fn is_running(&self, id: SessionId) -> bool {
        self.active_turns.read().await.contains_key(&id)
    }

    /// Shadow-git checkpoint manager for the workspace a session runs in.
    pub async fn checkpoint_manager_for(&self, dir: &Path) -> Result<Arc<CheckpointManager>> {
        let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if let Some(m) = self.checkpoint_managers.read().await.get(&key) {
            return Ok(Arc::clone(m));
        }
        let mut managers = self.checkpoint_managers.write().await;
        if let Some(m) = managers.get(&key) {
            return Ok(Arc::clone(m));
        }
        let manager = Arc::new(CheckpointManager::new(&key).await?);
        managers.insert(key, Arc::clone(&manager));
        Ok(manager)
    }

    pub async fn pending_permissions_for(&self, id: SessionId) -> Vec<PermissionRequest> {
        self.pending_permissions
            .read()
            .await
            .get(&id)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Permission manager with an independent mode for this session (forked from the default).
    pub async fn permission_manager_for(&self, id: SessionId) -> Arc<PermissionManager> {
        let mut managers = self.session_permissions.write().await;
        Arc::clone(
            managers
                .entry(id)
                .or_insert_with(|| Arc::new(self.permission_manager.fork())),
        )
    }

    /// Add a prompt to the session's prompt queue; returns the new queue length.
    pub async fn queue_prompt(&self, id: SessionId, prompt: String) -> usize {
        let mut queues = self.queued_prompts.write().await;
        let queue = queues.entry(id).or_default();
        queue.push_back(prompt);
        queue.len()
    }

    /// Pop the next queued prompt for this session, if any.
    pub async fn pop_queued_prompt(&self, id: SessionId) -> Option<String> {
        let mut queues = self.queued_prompts.write().await;
        let queue = queues.get_mut(&id)?;
        let next = queue.pop_front();
        if queue.is_empty() {
            queues.remove(&id);
        }
        next
    }

    /// Return all prompts currently queued for this session.
    pub async fn get_queued_prompts(&self, id: SessionId) -> Vec<String> {
        self.queued_prompts
            .read()
            .await
            .get(&id)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Clear all queued prompts for this session; returns how many were cleared.
    pub async fn clear_queued_prompts(&self, id: SessionId) -> usize {
        self.queued_prompts
            .write()
            .await
            .remove(&id)
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Drop all in-memory state for a session.
    pub async fn forget_session(&self, id: SessionId) {
        self.sessions.write().await.remove(&id);
        self.channels.write().await.remove(&id);
        self.pending_permissions.write().await.remove(&id);
        self.session_permissions.write().await.remove(&id);
        self.queued_prompts.write().await.remove(&id);
        if let Some(mut preview) = self.previews.lock().await.remove(&id) {
            let _ = preview.child.kill().await;
        }
    }
}
