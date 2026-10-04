use anyhow::Result;
use maukode_core::{AppConfig, PermissionManager, Session, SqliteSessionStore};
use maukode_protocol::{LLMEvent, PermissionRequest, SessionId};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;

/// Number of recent events kept per session so reconnecting clients can catch up.
const EVENT_LOG_CAPACITY: usize = 2000;

/// An event with a per-session monotonically increasing sequence number (SSE `id`).
#[derive(Debug, Clone, Serialize)]
pub struct SeqEvent {
    pub seq: u64,
    pub event: LLMEvent,
}

#[derive(Default)]
struct ChannelInner {
    last_seq: u64,
    log: VecDeque<SeqEvent>,
}

/// Broadcast channel plus a bounded replay log for one session.
pub struct SessionChannel {
    tx: broadcast::Sender<SeqEvent>,
    inner: std::sync::Mutex<ChannelInner>,
}

impl SessionChannel {
    fn new() -> Self {
        let (tx, _rx) = broadcast::channel(500);
        Self {
            tx,
            inner: std::sync::Mutex::new(ChannelInner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ChannelInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Append to the log and broadcast; returns the assigned sequence number.
    pub fn publish(&self, event: LLMEvent) -> u64 {
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
    pub fn replay_after(&self, after: u64) -> Vec<SeqEvent> {
        self.lock()
            .log
            .iter()
            .filter(|e| e.seq > after)
            .cloned()
            .collect()
    }

    /// Atomically snapshot the backlog (`seq > since`, if given) and subscribe to live events.
    /// Returns `(backlog, receiver, last_seq_seen)`; without `since` the backlog is empty.
    pub fn subscribe(
        &self,
        since: Option<u64>,
    ) -> (Vec<SeqEvent>, broadcast::Receiver<SeqEvent>, u64) {
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

pub struct ServerState {
    pub config: Arc<RwLock<AppConfig>>,
    pub store: Arc<SqliteSessionStore>,
    pub sessions: Arc<RwLock<HashMap<SessionId, Arc<RwLock<Session>>>>>,
    pub channels: Arc<RwLock<HashMap<SessionId, Arc<SessionChannel>>>>,
    pub pending_permissions: Arc<RwLock<HashMap<SessionId, HashMap<String, PermissionRequest>>>>,
    pub session_permissions: Arc<RwLock<HashMap<SessionId, Arc<PermissionManager>>>>,
    pub active_turns: Arc<RwLock<HashMap<SessionId, CancellationToken>>>,
    /// Default manager; per-session managers are forked from it (see `permission_manager_for`).
    pub permission_manager: Arc<PermissionManager>,
    pub checkpoint_manager: Arc<maukode_core::CheckpointManager>,
    pub mcp_manager: Arc<maukode_core::McpManager>,
    pub auth_token: Option<String>,
    pub default_dir: PathBuf,
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

        let checkpoint_manager = Arc::new(maukode_core::CheckpointManager::new(&dir).await?);
        let mcp_manager = Arc::new(maukode_core::McpManager::new());

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            store: Arc::new(store),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            pending_permissions: Arc::new(RwLock::new(HashMap::new())),
            session_permissions: Arc::new(RwLock::new(HashMap::new())),
            active_turns: Arc::new(RwLock::new(HashMap::new())),
            permission_manager,
            checkpoint_manager,
            mcp_manager,
            auth_token,
            default_dir: dir,
        })
    }

    pub async fn get_or_load_session(&self, id: SessionId) -> Result<Option<Arc<RwLock<Session>>>> {
        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.get(&id) {
            return Ok(Some(Arc::clone(s)));
        }

        if let Some(loaded) = self.store.load_session(id).await? {
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
        }
        self.channel(id).await.publish(event);
    }

    pub async fn resolve_pending_permission(&self, id: SessionId, request_id: &str) {
        let mut pending = self.pending_permissions.write().await;
        if let Some(map) = pending.get_mut(&id) {
            map.remove(request_id);
            if map.is_empty() {
                pending.remove(&id);
            }
        }
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

    /// Drop all in-memory state for a session.
    pub async fn forget_session(&self, id: SessionId) {
        self.sessions.write().await.remove(&id);
        self.channels.write().await.remove(&id);
        self.pending_permissions.write().await.remove(&id);
        self.session_permissions.write().await.remove(&id);
    }
}
