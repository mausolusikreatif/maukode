use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use maukode_protocol::{Message, SessionId, SessionState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use crate::session::Session;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub title: String,
    pub message_count: usize,
    pub provider: String,
    pub model: String,
    pub updated_at: DateTime<Utc>,
}

pub struct SqliteSessionStore {
    conn: Arc<Mutex<Connection>>,
    pub db_path: PathBuf,
}

impl SqliteSessionStore {
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }
    pub fn default_db_path() -> PathBuf {
        if let Some(mut dir) = dirs::data_dir() {
            dir.push("maukode");
            dir.push("maukode.db");
            dir
        } else if let Some(mut dir) = dirs::home_dir() {
            dir.push(".maukode");
            dir.push("maukode.db");
            dir
        } else {
            PathBuf::from(".maukode.db")
        }
    }

    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = path.into();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open sqlite database at {}", db_path.display()))?;

        // Initialize schema
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                provider TEXT,
                model TEXT,
                state TEXT,
                created_at TEXT,
                updated_at TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                parts_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id);
            ",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    pub async fn save_session(&self, session: &Session, provider: &str, model: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        let title = session
            .messages
            .first()
            .map(|m| {
                let text = m.text_content();
                if text.len() > 40 {
                    format!("{}...", &text[..40])
                } else {
                    text
                }
            })
            .unwrap_or_else(|| "New Session".to_string());

        let state_str = serde_json::to_string(&session.state)?;
        let id_str = session.id.to_string();
        let created_at_str = session.created_at.to_rfc3339();
        let updated_at_str = session.updated_at.to_rfc3339();

        conn.execute(
            "INSERT INTO sessions (id, title, provider, model, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                provider = excluded.provider,
                model = excluded.model,
                state = excluded.state,
                updated_at = excluded.updated_at;",
            params![
                id_str,
                title,
                provider,
                model,
                state_str,
                created_at_str,
                updated_at_str,
            ],
        )?;

        // Replace messages
        conn.execute("DELETE FROM messages WHERE session_id = ?1;", params![id_str])?;

        for msg in &session.messages {
            let role_str = serde_json::to_string(&msg.role)?;
            let parts_json = serde_json::to_string(&msg.parts)?;
            let now = Utc::now().to_rfc3339();

            conn.execute(
                "INSERT INTO messages (session_id, role, parts_json, created_at)
                 VALUES (?1, ?2, ?3, ?4);",
                params![id_str, role_str, parts_json, now],
            )?;
        }

        Ok(())
    }

    pub async fn load_session(&self, id: SessionId) -> Result<Option<Session>> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();

        let mut stmt = conn.prepare(
            "SELECT id, state, created_at, updated_at FROM sessions WHERE id = ?1;",
        )?;

        let session_meta = stmt.query_row(params![id_str], |row| {
            let id_str: String = row.get(0)?;
            let state_str: String = row.get(1)?;
            let created_at_str: String = row.get(2)?;
            let updated_at_str: String = row.get(3)?;
            Ok((id_str, state_str, created_at_str, updated_at_str))
        });

        let (_, state_str, created_at_str, updated_at_str) = match session_meta {
            Ok(data) => data,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let state: SessionState = serde_json::from_str(&state_str).unwrap_or(SessionState::Idle);
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        // Load messages
        let mut msg_stmt = conn.prepare(
            "SELECT role, parts_json FROM messages WHERE session_id = ?1 ORDER BY id ASC;",
        )?;

        let message_rows = msg_stmt.query_map(params![id_str], |row| {
            let role_str: String = row.get(0)?;
            let parts_json: String = row.get(1)?;
            Ok((role_str, parts_json))
        })?;

        let mut messages = Vec::new();
        for row in message_rows {
            if let Ok((role_str, parts_json)) = row {
                if let (Ok(role), Ok(parts)) = (
                    serde_json::from_str(&role_str),
                    serde_json::from_str(&parts_json),
                ) {
                    messages.push(Message { role, parts });
                }
            }
        }

        Ok(Some(Session {
            id,
            messages,
            state,
            created_at,
            updated_at,
        }))
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT 50;",
        )?;

        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let title: String = row.get(1).unwrap_or_default();
            let provider: String = row.get(2).unwrap_or_default();
            let model: String = row.get(3).unwrap_or_default();
            let updated_at_str: String = row.get(4)?;
            let msg_count: i64 = row.get(5).unwrap_or(0);
            Ok((id_str, title, provider, model, updated_at_str, msg_count))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            if let Ok((id_str, title, provider, model, updated_at_str, msg_count)) = row {
                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    summaries.push(SessionSummary {
                        id: SessionId(uuid),
                        title,
                        message_count: msg_count as usize,
                        provider,
                        model,
                        updated_at,
                    });
                }
            }
        }

        Ok(summaries)
    }

    pub async fn get_latest_session(&self) -> Result<Option<Session>> {
        let summaries = self.list_sessions().await?;
        if let Some(first) = summaries.first() {
            self.load_session(first.id).await
        } else {
            Ok(None)
        }
    }
}
