use crate::permission::PermissionMode;
use crate::session::Session;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use maukode_protocol::{Message, PermissionDecision, SessionId, SessionState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub directory: String,
    pub title: String,
    pub message_count: usize,
    pub provider: String,
    pub model: String,
    pub updated_at: DateTime<Utc>,
    /// Original repository when the session runs in an isolated git worktree.
    #[serde(default)]
    pub project_dir: Option<String>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAuditRecord {
    pub session_id: SessionId,
    pub turn_id: i64,
    pub tool_call_id: String,
    pub tool_name: String,
    pub action_key: String,
    pub mode: PermissionMode,
    pub decision: PermissionDecision,
    pub risk_level: String,
    pub result_visibility: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionModeAuditRecord {
    pub session_id: SessionId,
    pub turn_id: i64,
    pub previous_mode: PermissionMode,
    pub new_mode: PermissionMode,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionAuditRecord {
    pub session_id: SessionId,
    pub source_turn_start: i64,
    pub source_turn_end: i64,
    pub original_message_count: usize,
    pub compacted_message_count: usize,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub trigger: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

/// Structured provenance for a session, newest records first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReport {
    pub permission_count: usize,
    pub mode_change_count: usize,
    pub compaction_count: usize,
    pub permissions: Vec<PermissionAuditRecord>,
    pub mode_changes: Vec<PermissionModeAuditRecord>,
    pub compactions: Vec<CompactionAuditRecord>,
}

const AUDIT_REPORT_LIMIT: usize = 200;

fn parse_ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_default()
}

#[derive(Clone)]
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

        // Several agents (and CLI/TUI processes) share this file: WAL lets readers run during a
        // write and the busy timeout turns brief contention into a wait instead of an error.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;",
        )?;

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

            CREATE TABLE IF NOT EXISTS session_turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                ordinal INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(session_id, ordinal),
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_session_turns_session_id
                ON session_turns(session_id, id);
            CREATE TABLE IF NOT EXISTS permission_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn_id INTEGER NOT NULL,
                tool_call_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                action_key TEXT NOT NULL,
                mode TEXT NOT NULL,
                decision TEXT NOT NULL,
                risk_level TEXT NOT NULL,
                result_visibility TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_permission_audit_session_turn
                ON permission_audit(session_id, turn_id);

            CREATE TABLE IF NOT EXISTS permission_mode_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn_id INTEGER NOT NULL,
                previous_mode TEXT NOT NULL,
                new_mode TEXT NOT NULL,
                source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS compaction_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                source_turn_start INTEGER NOT NULL,
                source_turn_end INTEGER NOT NULL,
                original_message_count INTEGER NOT NULL,
                compacted_message_count INTEGER NOT NULL,
                original_tokens INTEGER NOT NULL,
                compacted_tokens INTEGER NOT NULL,
                trigger TEXT NOT NULL,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );
            ",
        )?;

        // Safe migration if table previously existed without directory column
        let _ = conn.execute(
            "ALTER TABLE sessions ADD COLUMN directory TEXT NOT NULL DEFAULT '';",
            [],
        );
        for ddl in [
            "ALTER TABLE sessions ADD COLUMN custom_title TEXT;",
            "ALTER TABLE sessions ADD COLUMN project_dir TEXT;",
            "ALTER TABLE sessions ADD COLUMN worktree_branch TEXT;",
            "ALTER TABLE sessions ADD COLUMN base_commit TEXT;",
        ] {
            let _ = conn.execute(ddl, []);
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_directory ON sessions(directory);",
            [],
        );

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    pub async fn save_session(&self, session: &Session, provider: &str, model: &str) -> Result<()> {
        let mut conn = self.conn.lock().await;

        let title = session
            .messages
            .first()
            .map(|m| {
                let text = m.text_content();
                if text.chars().count() > 40 {
                    format!("{}...", text.chars().take(40).collect::<String>())
                } else {
                    text
                }
            })
            .unwrap_or_else(|| "New Session".to_string());

        let tx = conn.transaction()?;
        let state_str = serde_json::to_string(&session.state)?;
        let id_str = session.id.to_string();
        let directory_str = session.directory.to_string_lossy().to_string();
        let created_at_str = session.created_at.to_rfc3339();
        let updated_at_str = session.updated_at.to_rfc3339();

        tx.execute(
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
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1;",
            params![id_str],
        )?;

        for msg in &session.messages {
            let role_str = serde_json::to_string(&msg.role)?;
            let parts_json = serde_json::to_string(&msg.parts)?;
            let now = Utc::now().to_rfc3339();

            tx.execute(
                "INSERT INTO messages (session_id, role, parts_json, created_at)
                 VALUES (?1, ?2, ?3, ?4);",
                params![id_str, role_str, parts_json, now],
            )?;

            // Retain full uncompressed message in messages_audit for historical auditing
            let _ = tx.execute(
                "INSERT OR IGNORE INTO messages_audit (session_id, role, parts_json, created_at)
                 VALUES (?1, ?2, ?3, ?4);",
                params![id_str, role_str, parts_json, now],
            );
        }

        tx.commit()?;
        Ok(())
    }

    pub async fn archive_session_messages(
        &self,
        session_id: SessionId,
        messages: &[Message],
    ) -> Result<()> {
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
        for (role_str, parts_json) in message_rows.flatten() {
            if let (Ok(role), Ok(parts)) = (
                serde_json::from_str(&role_str),
                serde_json::from_str(&parts_json),
            ) {
                messages.push(Message { role, parts });
            }
        }

        Ok(messages)
    }

    /// Set (or clear, with `None`) a user-chosen title. Returns whether the session exists.
    pub async fn set_session_title(&self, id: SessionId, title: Option<&str>) -> Result<bool> {
        let conn = self.conn.lock().await;
        let updated = conn.execute(
            "UPDATE sessions SET custom_title = ?1 WHERE id = ?2",
            params![title, id.to_string()],
        )?;
        Ok(updated > 0)
    }

    pub async fn session_title(&self, id: SessionId) -> Result<Option<String>> {
        let conn = self.conn.lock().await;
        let title = conn
            .query_row(
                "SELECT COALESCE(custom_title, title) FROM sessions WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        Ok(title.flatten())
    }

    /// Machine-readable audit data (permission decisions, mode changes, compactions).
    pub async fn audit_report(&self, session_id: SessionId) -> Result<AuditReport> {
        let conn = self.conn.lock().await;
        let id = session_id.to_string();
        let count = |table: &str| -> Result<usize> {
            let n: i64 = conn.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE session_id = ?1"),
                params![id],
                |row| row.get(0),
            )?;
            Ok(n as usize)
        };
        let mut report = AuditReport {
            permission_count: count("permission_audit")?,
            mode_change_count: count("permission_mode_audit")?,
            compaction_count: count("compaction_audit")?,
            ..Default::default()
        };

        let mut stmt = conn.prepare(
            "SELECT turn_id, tool_call_id, tool_name, action_key, mode, decision, risk_level,
                    result_visibility, created_at
             FROM permission_audit WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![id, AUDIT_REPORT_LIMIT as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;
        for row in rows {
            let (turn_id, tool_call_id, tool_name, action_key, mode, decision, risk, vis, ts) =
                row?;
            report.permissions.push(PermissionAuditRecord {
                session_id,
                turn_id,
                tool_call_id,
                tool_name,
                action_key,
                mode: serde_json::from_str(&mode)?,
                decision: serde_json::from_str(&decision)?,
                risk_level: risk,
                result_visibility: vis,
                created_at: parse_ts(&ts),
            });
        }

        let mut stmt = conn.prepare(
            "SELECT turn_id, previous_mode, new_mode, source, created_at
             FROM permission_mode_audit WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![id, AUDIT_REPORT_LIMIT as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (turn_id, previous, new, source, ts) = row?;
            report.mode_changes.push(PermissionModeAuditRecord {
                session_id,
                turn_id,
                previous_mode: serde_json::from_str(&previous)?,
                new_mode: serde_json::from_str(&new)?,
                source,
                created_at: parse_ts(&ts),
            });
        }

        let mut stmt = conn.prepare(
            "SELECT source_turn_start, source_turn_end, original_message_count,
                    compacted_message_count, original_tokens, compacted_tokens, trigger, summary,
                    created_at
             FROM compaction_audit WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![id, AUDIT_REPORT_LIMIT as i64], |row| {
            Ok(CompactionAuditRecord {
                session_id,
                source_turn_start: row.get(0)?,
                source_turn_end: row.get(1)?,
                original_message_count: row.get::<_, i64>(2)? as usize,
                compacted_message_count: row.get::<_, i64>(3)? as usize,
                original_tokens: row.get::<_, i64>(4)? as usize,
                compacted_tokens: row.get::<_, i64>(5)? as usize,
                trigger: row.get(6)?,
                summary: row.get(7)?,
                created_at: parse_ts(&row.get::<_, String>(8)?),
            })
        })?;
        for row in rows {
            report.compactions.push(row?);
        }
        Ok(report)
    }

    pub async fn set_worktree(
        &self,
        id: SessionId,
        info: &crate::worktree::WorktreeInfo,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE sessions SET project_dir = ?1, worktree_branch = ?2, base_commit = ?3
             WHERE id = ?4",
            params![
                info.project_dir.to_string_lossy(),
                info.branch,
                info.base_commit,
                id.to_string()
            ],
        )?;
        Ok(())
    }

    /// Worktree metadata for a session; the worktree path is the session's `directory`.
    pub async fn get_worktree(
        &self,
        id: SessionId,
    ) -> Result<Option<crate::worktree::WorktreeInfo>> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                "SELECT directory, project_dir, worktree_branch, base_commit
                 FROM sessions WHERE id = ?1",
                params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(match row {
            Some((path, Some(project), Some(branch), Some(base))) => {
                Some(crate::worktree::WorktreeInfo {
                    project_dir: PathBuf::from(project),
                    path: PathBuf::from(path),
                    branch,
                    base_commit: base,
                })
            }
            _ => None,
        })
    }

    /// The provider/model pinned to this session.
    pub async fn session_model(&self, id: SessionId) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().await;
        let row = conn
            .query_row(
                "SELECT provider, model FROM sessions WHERE id = ?1",
                params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()?;
        Ok(match row {
            Some((Some(p), Some(m))) if !p.is_empty() && !m.is_empty() => Some((p, m)),
            _ => None,
        })
    }

    pub async fn set_session_model(
        &self,
        id: SessionId,
        provider: &str,
        model: &str,
    ) -> Result<bool> {
        let conn = self.conn.lock().await;
        let n = conn.execute(
            "UPDATE sessions SET provider = ?1, model = ?2 WHERE id = ?3",
            params![provider, model, id.to_string()],
        )?;
        Ok(n > 0)
    }

    /// Delete a session and its dependent rows. Returns whether the session existed.
    pub async fn delete_session(&self, id: SessionId) -> Result<bool> {
        let mut conn = self.conn.lock().await;
        let id = id.to_string();
        let tx = conn.transaction()?;
        for table in [
            "messages",
            "messages_audit",
            "session_turns",
            "permission_audit",
            "permission_mode_audit",
            "compaction_audit",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE session_id = ?1"),
                params![id],
            )?;
        }
        let deleted = tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(deleted > 0)
    }

    pub async fn audit_summary(&self, session_id: SessionId) -> Result<String> {
        let conn = self.conn.lock().await;
        let id = session_id.to_string();
        let permissions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM permission_audit WHERE session_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        let mode_changes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM permission_mode_audit WHERE session_id = ?1",
            params![session_id.to_string()],
            |row| row.get(0),
        )?;
        let compactions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM compaction_audit WHERE session_id = ?1",
            params![session_id.to_string()],
            |row| row.get(0),
        )?;
        let mut output = format!(
            "Session provenance\n\nPermission decisions: {permissions}\nMode changes: {mode_changes}\nCompactions: {compactions}\n\nRecent permission decisions:\n"
        );
        let mut stmt = conn.prepare(
            "SELECT turn_id, tool_name, decision, result_visibility, created_at
             FROM permission_audit WHERE session_id = ?1 ORDER BY id DESC LIMIT 20",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |row| {
            Ok(format!(
                "turn {} | {} | {} | {} | {}",
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?
            ))
        })?;
        for row in rows {
            output.push_str(&format!("{}\n", row?));
        }
        Ok(output)
    }
    pub async fn start_turn(&self, session_id: SessionId, ordinal: i64) -> Result<i64> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR IGNORE INTO session_turns (session_id, ordinal, created_at)
             VALUES (?1, ?2, ?3)",
            params![session_id.to_string(), ordinal, Utc::now().to_rfc3339()],
        )?;
        let id = conn.query_row(
            "SELECT id FROM session_turns WHERE session_id = ?1 AND ordinal = ?2",
            params![session_id.to_string(), ordinal],
            |row| row.get(0),
        )?;
        Ok(id)
    }
    pub async fn record_permission(&self, record: &PermissionAuditRecord) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO permission_audit
             (session_id, turn_id, tool_call_id, tool_name, action_key, mode, decision,
              risk_level, result_visibility, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.session_id.to_string(),
                record.turn_id,
                record.tool_call_id,
                record.tool_name,
                record.action_key,
                serde_json::to_string(&record.mode)?,
                serde_json::to_string(&record.decision)?,
                record.risk_level,
                record.result_visibility,
                record.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub async fn record_permission_mode_change(
        &self,
        record: &PermissionModeAuditRecord,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO permission_mode_audit
             (session_id, turn_id, previous_mode, new_mode, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.session_id.to_string(),
                record.turn_id,
                serde_json::to_string(&record.previous_mode)?,
                serde_json::to_string(&record.new_mode)?,
                record.source,
                record.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub async fn record_compaction(&self, record: &CompactionAuditRecord) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO compaction_audit
             (session_id, source_turn_start, source_turn_end, original_message_count,
              compacted_message_count, original_tokens, compacted_tokens, trigger, summary,
              created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.session_id.to_string(),
                record.source_turn_start,
                record.source_turn_end,
                record.original_message_count as i64,
                record.compacted_message_count as i64,
                record.original_tokens as i64,
                record.compacted_tokens as i64,
                record.trigger,
                record.summary,
                record.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
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
        for (role_str, parts_json) in message_rows.flatten() {
            if let (Ok(role), Ok(parts)) = (
                serde_json::from_str(&role_str),
                serde_json::from_str(&parts_json),
            ) {
                messages.push(Message { role, parts });
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
            "SELECT s.id, s.directory, COALESCE(s.custom_title, s.title), s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count, s.project_dir, s.worktree_branch
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
            let project_dir: Option<String> = row.get(7).unwrap_or(None);
            let worktree_branch: Option<String> = row.get(8).unwrap_or(None);
            Ok((
                id_str,
                directory,
                title,
                provider,
                model,
                updated_at_str,
                msg_count,
                project_dir,
                worktree_branch,
            ))
        })?;

        let mut summaries = Vec::new();
        for (
            id_str,
            directory,
            title,
            provider,
            model,
            updated_at_str,
            msg_count,
            project_dir,
            worktree_branch,
        ) in rows.flatten()
        {
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
                    project_dir,
                    worktree_branch,
                });
            }
        }

        Ok(summaries)
    }

    pub async fn list_sessions_by_directory(&self, dir: &Path) -> Result<Vec<SessionSummary>> {
        let dir_str = dir.to_string_lossy().to_string();
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.directory, COALESCE(s.custom_title, s.title), s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count, s.project_dir, s.worktree_branch
             FROM sessions s
             LEFT JOIN messages m ON s.id = m.session_id
             WHERE s.directory = ?1 OR s.project_dir = ?1
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
            let project_dir: Option<String> = row.get(7).unwrap_or(None);
            let worktree_branch: Option<String> = row.get(8).unwrap_or(None);
            Ok((
                id_str,
                directory,
                title,
                provider,
                model,
                updated_at_str,
                msg_count,
                project_dir,
                worktree_branch,
            ))
        })?;

        let mut summaries = Vec::new();
        for (
            id_str,
            directory,
            title,
            provider,
            model,
            updated_at_str,
            msg_count,
            project_dir,
            worktree_branch,
        ) in rows.flatten()
        {
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
                    project_dir,
                    worktree_branch,
                });
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
        session.add_message(Message::assistant_text(
            "Turn 1 assistant response with 500 lines of build output",
        ));
        session.add_message(Message::user("Turn 2 user prompt"));
        session.add_message(Message::assistant_text("Turn 2 assistant response"));

        // Save session originally
        store
            .save_session(&session, "anthropic", "claude-3-7-sonnet")
            .await
            .unwrap();

        // Check audit messages count
        let audit_before = store.load_audit_messages(session.id).await.unwrap();
        assert_eq!(audit_before.len(), 4);

        // Now simulate compaction on active session
        session.messages = vec![
            Message::user("[CONVERSATION SUMMARY - COMPACTED CONTEXT]"),
            Message::assistant_text("Understood summary. Continuing..."),
        ];

        // Save compacted session
        store
            .save_session(&session, "anthropic", "claude-3-7-sonnet")
            .await
            .unwrap();

        // Active session messages has 2
        let loaded = store.load_session(session.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);

        // But audit table STILL has all original messages plus the new ones!
        let audit_after = store.load_audit_messages(session.id).await.unwrap();
        assert_eq!(audit_after.len(), 6);
        assert_eq!(
            audit_after[1].text_content(),
            "Turn 1 assistant response with 500 lines of build output"
        );
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_writers_across_connections_do_not_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        // Two stores on one file stand in for the daemon plus a local CLI/TUI process.
        let stores = [
            SqliteSessionStore::new(&path).unwrap(),
            SqliteSessionStore::new(&path).unwrap(),
        ];
        let mut tasks = Vec::new();
        for i in 0..40 {
            let store = stores[i % 2].clone();
            tasks.push(tokio::spawn(async move {
                let mut session = Session::new("/tmp/x");
                for n in 0..20 {
                    session.add_message(Message::user(format!("m{n}")));
                }
                store.save_session(&session, "p", "m").await?;
                store.save_session(&session, "p", "m").await?;
                anyhow::Ok(session.id)
            }));
        }
        for t in tasks {
            let id = t.await.unwrap().unwrap();
            let loaded = stores[0].load_session(id).await.unwrap().unwrap();
            assert_eq!(loaded.messages.len(), 20);
        }
        let mode: String = stores[0]
            .conn
            .lock()
            .await
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[tokio::test]
    async fn delete_session_removes_dependents_and_worktree_metadata_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();
        let mut session = Session::new("/tmp/wt");
        session.add_message(Message::user("hi"));
        store.save_session(&session, "p", "m").await.unwrap();
        let info = crate::worktree::WorktreeInfo {
            project_dir: "/repo".into(),
            path: "/tmp/wt".into(),
            branch: "maukode/abc".into(),
            base_commit: "deadbeef".into(),
        };
        store.set_worktree(session.id, &info).await.unwrap();
        assert_eq!(store.get_worktree(session.id).await.unwrap(), Some(info));
        let listed = store
            .list_sessions_by_directory(Path::new("/repo"))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].worktree_branch.as_deref(), Some("maukode/abc"));
        assert!(store.delete_session(session.id).await.unwrap());
        assert!(store.load_session(session.id).await.unwrap().is_none());
        assert!(!store.delete_session(session.id).await.unwrap());
    }
}
