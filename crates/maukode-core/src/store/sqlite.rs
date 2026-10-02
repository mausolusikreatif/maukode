use std::path::{Path, PathBuf};
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
    pub directory: String,
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
                directory TEXT NOT NULL DEFAULT '',
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

            CREATE TABLE IF NOT EXISTS messages_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                parts_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(session_id, parts_json)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_audit_session_id ON messages_audit(session_id);
            ",
        )?;

        // Safe migration if table previously existed without directory column
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN directory TEXT NOT NULL DEFAULT '';", []);
        let _ = conn.execute("CREATE INDEX IF NOT EXISTS idx_sessions_directory ON sessions(directory);", []);

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
        let directory_str = session.directory.to_string_lossy().to_string();
        let created_at_str = session.created_at.to_rfc3339();
        let updated_at_str = session.updated_at.to_rfc3339();

        conn.execute(
            "INSERT INTO sessions (id, directory, title, provider, model, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                directory = excluded.directory,
                title = excluded.title,
                provider = excluded.provider,
                model = excluded.model,
                state = excluded.state,
                updated_at = excluded.updated_at;",
            params![
                id_str,
                directory_str,
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

            // Retain full uncompressed message in messages_audit for historical auditing
            let _ = conn.execute(
                "INSERT OR IGNORE INTO messages_audit (session_id, role, parts_json, created_at)
                 VALUES (?1, ?2, ?3, ?4);",
                params![id_str, role_str, parts_json, now],
            );
        }

        Ok(())
    }

    pub async fn archive_session_messages(&self, session_id: SessionId, messages: &[Message]) -> Result<()> {
        let conn = self.conn.lock().await;
        let id_str = session_id.to_string();
        for msg in messages {
            let role_str = serde_json::to_string(&msg.role)?;
            let parts_json = serde_json::to_string(&msg.parts)?;
            let now = Utc::now().to_rfc3339();

            let _ = conn.execute(
                "INSERT OR IGNORE INTO messages_audit (session_id, role, parts_json, created_at)
                 VALUES (?1, ?2, ?3, ?4);",
                params![id_str, role_str, parts_json, now],
            );
        }
        Ok(())
    }

    pub async fn load_audit_messages(&self, id: SessionId) -> Result<Vec<Message>> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();

        let mut stmt = conn.prepare(
            "SELECT role, parts_json FROM messages_audit WHERE session_id = ?1 ORDER BY id ASC;",
        )?;

        let message_rows = stmt.query_map(params![id_str], |row| {
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

        Ok(messages)
    }

    pub async fn load_session(&self, id: SessionId) -> Result<Option<Session>> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();

        let mut stmt = conn.prepare(
            "SELECT id, directory, state, created_at, updated_at FROM sessions WHERE id = ?1;",
        )?;

        let session_meta = stmt.query_row(params![id_str], |row| {
            let id_str: String = row.get(0)?;
            let dir_str: String = row.get(1).unwrap_or_default();
            let state_str: String = row.get(2)?;
            let created_at_str: String = row.get(3)?;
            let updated_at_str: String = row.get(4)?;
            Ok((id_str, dir_str, state_str, created_at_str, updated_at_str))
        });

        let (_, dir_str, state_str, created_at_str, updated_at_str) = match session_meta {
            Ok(data) => data,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let directory = if dir_str.is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(dir_str)
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
            directory,
            messages,
            state,
            created_at,
            updated_at,
        }))
    }

    pub async fn list_all_sessions(&self) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.directory, s.title, s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT 100;",
        )?;

        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let directory: String = row.get(1).unwrap_or_default();
            let title: String = row.get(2).unwrap_or_default();
            let provider: String = row.get(3).unwrap_or_default();
            let model: String = row.get(4).unwrap_or_default();
            let updated_at_str: String = row.get(5)?;
            let msg_count: i64 = row.get(6).unwrap_or(0);
            Ok((id_str, directory, title, provider, model, updated_at_str, msg_count))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            if let Ok((id_str, directory, title, provider, model, updated_at_str, msg_count)) = row {
                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    summaries.push(SessionSummary {
                        id: SessionId(uuid),
                        directory,
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

    pub async fn list_sessions_by_directory(&self, dir: &Path) -> Result<Vec<SessionSummary>> {
        let dir_str = dir.to_string_lossy().to_string();
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.directory, s.title, s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             WHERE s.directory = ?1
             GROUP BY s.id
             ORDER BY s.updated_at DESC
             LIMIT 50;",
        )?;

        let rows = stmt.query_map(params![dir_str], |row| {
            let id_str: String = row.get(0)?;
            let directory: String = row.get(1).unwrap_or_default();
            let title: String = row.get(2).unwrap_or_default();
            let provider: String = row.get(3).unwrap_or_default();
            let model: String = row.get(4).unwrap_or_default();
            let updated_at_str: String = row.get(5)?;
            let msg_count: i64 = row.get(6).unwrap_or(0);
            Ok((id_str, directory, title, provider, model, updated_at_str, msg_count))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            if let Ok((id_str, directory, title, provider, model, updated_at_str, msg_count)) = row {
                if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    summaries.push(SessionSummary {
                        id: SessionId(uuid),
                        directory,
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

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        self.list_all_sessions().await
    }

    pub async fn get_latest_session(&self) -> Result<Option<Session>> {
        let summaries = self.list_all_sessions().await?;
        if let Some(first) = summaries.first() {
            self.load_session(first.id).await
        } else {
            Ok(None)
        }
    }

    pub async fn get_latest_session_in_directory(&self, dir: &Path) -> Result<Option<Session>> {
        let summaries = self.list_sessions_by_directory(dir).await?;
        if let Some(first) = summaries.first() {
            self.load_session(first.id).await
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sqlite_retains_uncompressed_audit_messages_after_compaction() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_audit.db");
        let store = SqliteSessionStore::new(&db_path).unwrap();

        let mut session = Session::new(temp_dir.path());
        session.add_message(Message::user("Turn 1 user prompt with lots of context"));
        session.add_message(Message::assistant_text("Turn 1 assistant response with 500 lines of build output"));
        session.add_message(Message::user("Turn 2 user prompt"));
        session.add_message(Message::assistant_text("Turn 2 assistant response"));

        // Save session originally
        store.save_session(&session, "anthropic", "claude-3-7-sonnet").await.unwrap();

        // Check audit messages count
        let audit_before = store.load_audit_messages(session.id).await.unwrap();
        assert_eq!(audit_before.len(), 4);

        // Now simulate compaction on active session
        session.messages = vec![
            Message::user("[CONVERSATION SUMMARY - COMPACTED CONTEXT]"),
            Message::assistant_text("Understood summary. Continuing..."),
        ];

        // Save compacted session
        store.save_session(&session, "anthropic", "claude-3-7-sonnet").await.unwrap();

        // Active session messages has 2
        let loaded = store.load_session(session.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);

        // But audit table STILL has all original messages plus the new ones!
        let audit_after = store.load_audit_messages(session.id).await.unwrap();
        assert_eq!(audit_after.len(), 6);
        assert_eq!(audit_after[1].text_content(), "Turn 1 assistant response with 500 lines of build output");
    }
}

