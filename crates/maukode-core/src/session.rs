use chrono::{DateTime, Utc};
use maukode_protocol::{Message, SessionId, SessionMetadata, SessionState};

pub struct Session {
    pub id: SessionId,
    pub messages: Vec<Message>,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            messages: Vec::new(),
            state: SessionState::Idle,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_id(id: SessionId) -> Self {
        let now = Utc::now();
        Self {
            id,
            messages: Vec::new(),
            state: SessionState::Idle,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }

    pub fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            id: self.id,
            title: None,
            created_at: self.created_at,
            updated_at: self.updated_at,
            state: self.state.clone(),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
