pub mod sqlite;

use crate::session::Session;
use async_trait::async_trait;
use maukode_protocol::SessionId;
pub use sqlite::{SessionSummary, SqliteSessionStore};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, id: SessionId) -> Option<Arc<RwLock<Session>>>;
    async fn save(&self, session: Session) -> Arc<RwLock<Session>>;
}

#[derive(Default, Clone)]
pub struct InMemorySessionStore {
    sessions: Arc<RwLock<HashMap<SessionId, Arc<RwLock<Session>>>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn get(&self, id: SessionId) -> Option<Arc<RwLock<Session>>> {
        let lock = self.sessions.read().await;
        lock.get(&id).cloned()
    }

    async fn save(&self, session: Session) -> Arc<RwLock<Session>> {
        let id = session.id;
        let mut lock = self.sessions.write().await;
        let entry = Arc::new(RwLock::new(session));
        lock.insert(id, Arc::clone(&entry));
        entry
    }
}
