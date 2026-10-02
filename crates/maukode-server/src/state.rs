use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use maukode_core::{AppConfig, Session, SqliteSessionStore};
use maukode_protocol::{LLMEvent, SessionId};
use tokio::sync::{broadcast, RwLock};

pub struct ServerState {
    pub config: Arc<RwLock<AppConfig>>,
    pub store: Arc<SqliteSessionStore>,
    pub sessions: Arc<RwLock<HashMap<SessionId, Arc<RwLock<Session>>>>>,
    pub event_broadcasters: Arc<RwLock<HashMap<SessionId, broadcast::Sender<LLMEvent>>>>,
    pub auth_token: Option<String>,
    pub default_dir: PathBuf,
}

impl ServerState {
    pub fn new(auth_token: Option<String>, default_dir: Option<PathBuf>) -> Result<Self> {
        let config = AppConfig::load();
        let store = SqliteSessionStore::new(SqliteSessionStore::default_db_path())?;
        let dir = default_dir.unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        });

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            store: Arc::new(store),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcasters: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn get_event_sender(&self, id: SessionId) -> broadcast::Sender<LLMEvent> {
        let mut broadcasters = self.event_broadcasters.write().await;
        if let Some(sender) = broadcasters.get(&id) {
            sender.clone()
        } else {
            let (tx, _rx) = broadcast::channel(500);
            broadcasters.insert(id, tx.clone());
            tx
        }
    }
}
