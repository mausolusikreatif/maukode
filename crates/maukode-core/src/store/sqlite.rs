use crate::permission::PermissionMode;
use crate::session::Session;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use maukode_protocol::{Message, PermissionDecision, SessionId, SessionState, SessionUsage};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
    /// Hidden from the default session list (`PUT /sessions/{id}/archive`).
    #[serde(default)]
    pub archived: bool,
    /// A chat-only session: it works in a private empty folder and belongs to no project.
    #[serde(default)]
    pub chat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PermissionAuditRecord {
    pub session_id: SessionId,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PermissionModeAuditRecord {
    pub session_id: SessionId,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub turn_id: i64,
    pub previous_mode: PermissionMode,
    pub new_mode: PermissionMode,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CompactionAuditRecord {
    pub session_id: SessionId,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub source_turn_start: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub source_turn_end: i64,
    pub original_message_count: usize,
    pub compacted_message_count: usize,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub trigger: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(
                f.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

/// Retry an operation that hit lock contention, backing off for up to ~10 seconds.
fn retry_busy<T>(mut op: impl FnMut() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut delay = std::time::Duration::from_millis(5);
    loop {
        match op() {
            Err(e) if is_busy(&e) && std::time::Instant::now() < deadline => {
                std::thread::sleep(delay);
                delay = (delay * 2).min(std::time::Duration::from_millis(100));
            }
            result => return result,
        }
    }
}

/// Stable identity of a stored message: SHA-256 of its serialized role and parts.
fn read_session_usage(conn: &Connection, id: SessionId) -> rusqlite::Result<SessionUsage> {
    let usage = conn
        .query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
             cost_microusd, unpriced_responses FROM session_usage WHERE session_id = ?1",
            params![id.to_string()],
            |row| {
                let get = |i: usize| row.get::<_, i64>(i).map(|v| v.max(0) as u64);
                Ok(SessionUsage {
                    input_tokens: get(0)?,
                    output_tokens: get(1)?,
                    cache_read_tokens: get(2)?,
                    cache_write_tokens: get(3)?,
                    cost_microusd: get(4)?,
                    unpriced_responses: get(5)?,
                    context_tokens: 0,
                })
            },
        )
        .optional()?;
    let mut usage = usage.unwrap_or_default();
    usage.context_tokens = conn
        .query_row(
            "SELECT input_tokens FROM usage_requests WHERE session_id = ?1 \
             ORDER BY id DESC LIMIT 1",
            params![id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map_or(0, |v| v.max(0) as u64);
    Ok(usage)
}

fn count_at(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u64> {
    Ok(row.get::<_, i64>(i)?.max(0) as u64)
}

fn opt_count_at(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<u64>> {
    Ok(row.get::<_, Option<i64>>(i)?.map(|v| v.max(0) as u64))
}

/// The system prompt and tool schemas (a JSON array) that requests were sent with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptSnapshot {
    pub hash: String,
    pub system_prompt: String,
    pub tools_json: String,
    pub created_at: String,
}

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromptSnapshot> {
    Ok(PromptSnapshot {
        hash: row.get(0)?,
        system_prompt: row.get(1)?,
        tools_json: row.get(2)?,
        created_at: row.get(3)?,
    })
}

/// Identity of a prompt/tool-schema pair: SHA-256 over both, separated by a NUL byte.
fn prompt_snapshot_hash(system_prompt: &str, tools_json: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(system_prompt.as_bytes());
    hasher.update([0u8]);
    hasher.update(tools_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// One request the main agent sent to the model, as stored in `usage_requests`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRequestRecord {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub snapshot_hash: Option<String>,
    pub created_at: String,
}

fn content_hash(role_json: &str, parts_json: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(role_json.as_bytes());
    hasher.update([0]);
    hasher.update(parts_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Older databases deduplicated `messages_audit` with `UNIQUE(session_id, parts_json)`, which
/// indexes the full text of every message. Rebuild such a table keyed by content hash instead.
fn migrate_messages_audit(conn: &mut Connection) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let migrated = tx
        .prepare("SELECT 1 FROM pragma_table_info('messages_audit') WHERE name = 'content_hash';")?
        .exists([])?;
    if migrated {
        return tx.commit();
    }
    tx.execute_batch(
        "CREATE TABLE messages_audit_v2 (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            parts_json TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(session_id, content_hash)
        );",
    )?;
    {
        let mut read = tx.prepare(
            "SELECT session_id, role, parts_json, created_at FROM messages_audit ORDER BY id;",
        )?;
        let mut write = tx.prepare(
            "INSERT OR IGNORE INTO messages_audit_v2
                (session_id, role, parts_json, content_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5);",
        )?;
        let mut rows = read.query([])?;
        while let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let role: String = row.get(1)?;
            let parts: String = row.get(2)?;
            let created_at: String = row.get(3)?;
            let hash = content_hash(&role, &parts);
            write.execute(params![session_id, role, parts, hash, created_at])?;
        }
    }
    tx.execute_batch(
        "DROP TABLE messages_audit;
         ALTER TABLE messages_audit_v2 RENAME TO messages_audit;
         CREATE INDEX IF NOT EXISTS idx_messages_audit_session_id ON messages_audit(session_id);",
    )?;
    tx.commit()
}

/// Structured provenance for a session, newest records first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
        if let Some(mut dir) = crate::paths::data_dir() {
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

        let mut conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open sqlite database at {}", db_path.display()))?;

        // Several agents (and CLI/TUI processes) share this file: WAL lets readers run during a
        // write and the busy timeout turns brief contention into a wait instead of an error.
        // The timeout must be installed first: switching to WAL takes a lock, and without a busy
        // handler a concurrent opener fails immediately with "database is locked".
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Even with a busy handler SQLite reports SQLITE_BUSY at once for some lock upgrades
        // (it cannot wait without risking deadlock), so setup is retried explicitly.
        retry_busy(|| {
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        })?;

        // Initialize schema
        retry_busy(|| {
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
                content_hash TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id);

            CREATE TABLE IF NOT EXISTS messages_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                parts_json TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(session_id, content_hash)
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

            CREATE TABLE IF NOT EXISTS session_usage (
                session_id TEXT PRIMARY KEY,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens INTEGER NOT NULL DEFAULT 0,
                cost_microusd INTEGER NOT NULL DEFAULT 0,
                unpriced_responses INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            -- One row per request the main agent sent. No foreign key: the runner records a
            -- request before every front end has saved its session row.
            CREATE TABLE IF NOT EXISTS usage_requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                snapshot_hash TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_requests_session_id
                ON usage_requests(session_id, id);

            -- The system prompt and tool schemas of requests, stored once per distinct pair.
            CREATE TABLE IF NOT EXISTS prompt_snapshots (
                hash TEXT PRIMARY KEY,
                system_prompt TEXT NOT NULL,
                tools_json TEXT NOT NULL,
                created_at TEXT NOT NULL
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
            )
        })?;

        // Safe migrations for databases created by older versions. "duplicate column" just means
        // the migration already ran; anything else (other than lock contention) is a real error.
        for ddl in [
            "ALTER TABLE sessions ADD COLUMN directory TEXT NOT NULL DEFAULT '';",
            "ALTER TABLE sessions ADD COLUMN custom_title TEXT;",
            "ALTER TABLE sessions ADD COLUMN project_dir TEXT;",
            "ALTER TABLE sessions ADD COLUMN worktree_branch TEXT;",
            "ALTER TABLE sessions ADD COLUMN base_commit TEXT;",
            "ALTER TABLE messages ADD COLUMN content_hash TEXT;",
            "ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;",
            "ALTER TABLE usage_requests ADD COLUMN snapshot_hash TEXT;",
        ] {
            match retry_busy(|| conn.execute(ddl, [])) {
                Ok(_) => {}
                Err(e) if e.to_string().contains("duplicate column name") => {}
                Err(e) => return Err(e.into()),
            }
        }
        retry_busy(|| {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_sessions_directory ON sessions(directory);",
                [],
            )
        })?;
        retry_busy(|| migrate_messages_audit(&mut conn))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Persist the session. Only messages that changed since the last save are written: stored
    /// rows are compared by content hash, rows after the first difference are replaced, and
    /// unchanged rows keep their `created_at`. Runs on a blocking thread.
    pub async fn save_session(&self, session: &Session, provider: &str, model: &str) -> Result<()> {
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

        let rows = session
            .messages
            .iter()
            .map(|msg| {
                let role = serde_json::to_string(&msg.role)?;
                let parts = serde_json::to_string(&msg.parts)?;
                let hash = content_hash(&role, &parts);
                Ok((role, parts, hash))
            })
            .collect::<Result<Vec<_>>>()?;
        let state_str = serde_json::to_string(&session.state)?;
        let id_str = session.id.to_string();
        let directory_str = session.directory.to_string_lossy().to_string();
        let created_at_str = session.created_at.to_rfc3339();
        let updated_at_str = session.updated_at.to_rfc3339();
        let provider = provider.to_string();
        let model = model.to_string();

        let mut conn = Arc::clone(&self.conn).lock_owned().await;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let tx = conn.transaction()?;
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

            let stored: Vec<(i64, Option<String>)> = {
                let mut stmt = tx.prepare(
                    "SELECT id, content_hash FROM messages WHERE session_id = ?1 ORDER BY id ASC;",
                )?;
                let stored = stmt.query_map(params![id_str], |row| Ok((row.get(0)?, row.get(1)?)))?;
                stored.collect::<rusqlite::Result<_>>()?
            };
            let kept = stored
                .iter()
                .zip(&rows)
                .take_while(|((_, stored_hash), (_, _, hash))| {
                    stored_hash.as_deref() == Some(hash.as_str())
                })
                .count();
            if let Some((first_stale, _)) = stored.get(kept) {
                tx.execute(
                    "DELETE FROM messages WHERE session_id = ?1 AND id >= ?2;",
                    params![id_str, first_stale],
                )?;
            }

            let now = Utc::now().to_rfc3339();
            for (role, parts, hash) in &rows[kept..] {
                tx.execute(
                    "INSERT INTO messages (session_id, role, parts_json, content_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                    params![id_str, role, parts, hash, now],
                )?;
                // Keep every message ever stored, so compaction never loses the original history.
                tx.execute(
                    "INSERT OR IGNORE INTO messages_audit
                        (session_id, role, parts_json, content_hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5);",
                    params![id_str, role, parts, hash, now],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .context("save_session task failed")?
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
            let hash = content_hash(&role_str, &parts_json);
            let now = Utc::now().to_rfc3339();

            let _ = conn.execute(
                "INSERT OR IGNORE INTO messages_audit
                    (session_id, role, parts_json, content_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5);",
                params![id_str, role_str, parts_json, hash, now],
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

    /// Add `delta` to the session's usage totals and return the new totals.
    pub async fn add_session_usage(
        &self,
        id: SessionId,
        delta: &SessionUsage,
    ) -> Result<SessionUsage> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO session_usage (session_id, input_tokens, output_tokens, \
             cache_read_tokens, cache_write_tokens, cost_microusd, unpriced_responses) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(session_id) DO UPDATE SET \
             input_tokens = input_tokens + excluded.input_tokens, \
             output_tokens = output_tokens + excluded.output_tokens, \
             cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens, \
             cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens, \
             cost_microusd = cost_microusd + excluded.cost_microusd, \
             unpriced_responses = unpriced_responses + excluded.unpriced_responses",
            params![
                id.to_string(),
                delta.input_tokens as i64,
                delta.output_tokens as i64,
                delta.cache_read_tokens as i64,
                delta.cache_write_tokens as i64,
                delta.cost_microusd as i64,
                delta.unpriced_responses as i64,
            ],
        )?;
        Ok(read_session_usage(&conn, id)?)
    }

    /// Record one request of the main agent, for the per-request series and the current context
    /// size. Subagent requests are only part of the session totals.
    pub async fn record_usage_request(
        &self,
        id: SessionId,
        model: &str,
        usage: &maukode_protocol::TokenUsage,
        snapshot_hash: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO usage_requests (session_id, model, input_tokens, output_tokens, \
             cache_read_tokens, cache_write_tokens, snapshot_hash, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id.to_string(),
                model,
                usage.input_tokens as i64,
                usage.output_tokens as i64,
                usage.cache_read_tokens.map(|v| v as i64),
                usage.cache_write_tokens.map(|v| v as i64),
                snapshot_hash,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Store a system prompt and tool schemas once, however many requests used them, and return
    /// the hash that identifies the pair.
    pub async fn record_prompt_snapshot(
        &self,
        system_prompt: &str,
        tools_json: &str,
    ) -> Result<String> {
        let hash = prompt_snapshot_hash(system_prompt, tools_json);
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR IGNORE INTO prompt_snapshots (hash, system_prompt, tools_json, \
             created_at) VALUES (?1, ?2, ?3, ?4)",
            params![hash, system_prompt, tools_json, Utc::now().to_rfc3339()],
        )?;
        Ok(hash)
    }

    /// Whether any recorded request of the session was served partly from the provider's cache.
    pub async fn session_has_cache_hits(&self, id: SessionId) -> Result<bool> {
        let conn = self.conn.lock().await;
        let hit = conn
            .query_row(
                "SELECT 1 FROM usage_requests WHERE session_id = ?1 \
                 AND cache_read_tokens > 0 LIMIT 1",
                params![id.to_string()],
                |_| Ok(()),
            )
            .optional()?;
        Ok(hit.is_some())
    }

    /// A stored prompt/tool-schema pair by its hash.
    pub async fn prompt_snapshot(&self, hash: &str) -> Result<Option<PromptSnapshot>> {
        let conn = self.conn.lock().await;
        let snapshot = conn
            .query_row(
                "SELECT hash, system_prompt, tools_json, created_at FROM prompt_snapshots \
                 WHERE hash = ?1",
                params![hash],
                snapshot_from_row,
            )
            .optional()?;
        Ok(snapshot)
    }

    /// The prompt and tool schemas of the session's latest recorded request.
    pub async fn latest_prompt_snapshot(&self, id: SessionId) -> Result<Option<PromptSnapshot>> {
        let conn = self.conn.lock().await;
        let snapshot = conn
            .query_row(
                "SELECT s.hash, s.system_prompt, s.tools_json, s.created_at \
                 FROM usage_requests r JOIN prompt_snapshots s ON s.hash = r.snapshot_hash \
                 WHERE r.session_id = ?1 ORDER BY r.id DESC LIMIT 1",
                params![id.to_string()],
                snapshot_from_row,
            )
            .optional()?;
        Ok(snapshot)
    }

    /// The session's requests in the order they were sent.
    pub async fn session_usage_requests(&self, id: SessionId) -> Result<Vec<UsageRequestRecord>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
             snapshot_hash, created_at FROM usage_requests WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![id.to_string()], |row| {
            Ok(UsageRequestRecord {
                model: row.get(0)?,
                input_tokens: count_at(row, 1)?,
                output_tokens: count_at(row, 2)?,
                cache_read_tokens: opt_count_at(row, 3)?,
                cache_write_tokens: opt_count_at(row, 4)?,
                snapshot_hash: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The session's usage totals (zero for a session without any).
    pub async fn session_usage(&self, id: SessionId) -> Result<SessionUsage> {
        let conn = self.conn.lock().await;
        Ok(read_session_usage(&conn, id)?)
    }

    /// Archive or restore a session. Returns whether it exists.
    pub async fn set_session_archived(&self, id: SessionId, archived: bool) -> Result<bool> {
        let conn = self.conn.lock().await;
        let n = conn.execute(
            "UPDATE sessions SET archived = ?1 WHERE id = ?2",
            params![archived as i64, id.to_string()],
        )?;
        Ok(n > 0)
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
            "session_usage",
            "usage_requests",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE session_id = ?1"),
                params![id],
            )?;
        }
        tx.execute(
            "DELETE FROM prompt_snapshots WHERE hash NOT IN \
             (SELECT snapshot_hash FROM usage_requests WHERE snapshot_hash IS NOT NULL)",
            [],
        )?;
        let deleted = tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(deleted > 0)
    }

    /// Remove every session that has no messages left in the `messages` table.
    ///
    /// Sessions are created (and persisted) eagerly when a client opens one, but a session the
    /// user never actually talked to leaves an empty row behind forever. This sweeps those orphan
    /// rows — including their audit/turn rows — and returns how many were removed. A `worktree`
    /// is only dropped from metadata here; the on-disk checkout is torn down by the caller.
    pub async fn delete_empty_sessions(&self) -> Result<usize> {
        let mut conn = self.conn.lock().await;
        let tx = conn.transaction()?;
        // A subquery keeps this independent of any bind-variable limit (SQLite caps
        // SQLITE_MAX_VARIABLE_NUMBER, and 999 empty sessions would otherwise overflow it). The
        // `sessions` table gets no alias because SQLite forbids aliases in DELETE statements.
        let empty_sub = "NOT EXISTS (SELECT 1 FROM messages m WHERE m.session_id = sessions.id)";
        let empty_in = "NOT EXISTS (SELECT 1 FROM messages m WHERE m.session_id = s.id)";
        for table in [
            "messages",
            "messages_audit",
            "session_turns",
            "permission_audit",
            "permission_mode_audit",
            "compaction_audit",
            "session_usage",
        ] {
            tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE session_id IN \
                     (SELECT id FROM sessions s WHERE {empty_in})"
                ),
                [],
            )?;
        }
        let removed = tx.execute(&format!("DELETE FROM sessions WHERE {empty_sub}"), [])?;
        tx.commit()?;
        Ok(removed)
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
            system_prompt: None,
        }))
    }

    pub async fn list_all_sessions(&self) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.directory, COALESCE(s.custom_title, s.title), s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count, s.project_dir, s.worktree_branch, s.archived
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
            let archived: bool = row.get::<_, i64>(9).map(|v| v != 0).unwrap_or(false);
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
                archived,
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
            archived,
        ) in rows.flatten()
        {
            if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let chat = crate::paths::is_chat_dir(Path::new(&directory));
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
                    archived,
                    chat,
                });
            }
        }

        Ok(summaries)
    }

    pub async fn list_sessions_by_directory(&self, dir: &Path) -> Result<Vec<SessionSummary>> {
        let dir_str = dir.to_string_lossy().to_string();
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.directory, COALESCE(s.custom_title, s.title), s.provider, s.model, s.updated_at, COUNT(m.id) as msg_count, s.project_dir, s.worktree_branch, s.archived
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
            let archived: bool = row.get::<_, i64>(9).map(|v| v != 0).unwrap_or(false);
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
                archived,
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
            archived,
        ) in rows.flatten()
        {
            if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let chat = crate::paths::is_chat_dir(Path::new(&directory));
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
                    archived,
                    chat,
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

    #[tokio::test]
    async fn session_usage_accumulates_and_goes_with_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();
        let mut session = Session::new(dir.path());
        session.add_message(Message::user("first"));
        store.save_session(&session, "p", "m").await.unwrap();
        assert_eq!(
            store.session_usage(session.id).await.unwrap(),
            SessionUsage::default()
        );

        let delta = SessionUsage {
            input_tokens: 100,
            output_tokens: 10,
            cache_read_tokens: 80,
            cache_write_tokens: 0,
            cost_microusd: 500,
            unpriced_responses: 0,
            context_tokens: 0,
        };
        store.add_session_usage(session.id, &delta).await.unwrap();
        let unpriced = SessionUsage {
            input_tokens: 5,
            unpriced_responses: 1,
            ..SessionUsage::default()
        };
        let total = store
            .add_session_usage(session.id, &unpriced)
            .await
            .unwrap();
        assert_eq!(total.input_tokens, 105);
        assert_eq!(total.cost_microusd, 500);
        assert_eq!(total.unpriced_responses, 1);
        assert_eq!(store.session_usage(session.id).await.unwrap(), total);

        store.delete_session(session.id).await.unwrap();
        assert_eq!(
            store.session_usage(session.id).await.unwrap(),
            SessionUsage::default()
        );
    }

    #[tokio::test]
    async fn requests_are_recorded_and_the_latest_sets_the_context_size() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();
        let mut session = Session::new(dir.path());
        session.add_message(Message::user("first"));
        store.save_session(&session, "p", "m").await.unwrap();
        let usage = store.session_usage(session.id).await.unwrap();
        assert_eq!(usage.context_tokens, 0);

        let first = maukode_protocol::TokenUsage {
            input_tokens: 1_000,
            output_tokens: 20,
            cache_read_tokens: Some(800),
            cache_write_tokens: None,
        };
        let second = maukode_protocol::TokenUsage {
            input_tokens: 1_500,
            output_tokens: 30,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };
        store
            .record_usage_request(session.id, "m", &first, None)
            .await
            .unwrap();
        store
            .record_usage_request(session.id, "m", &second, None)
            .await
            .unwrap();

        let requests = store.session_usage_requests(session.id).await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].input_tokens, 1_000);
        assert_eq!(requests[0].cache_read_tokens, Some(800));
        assert_eq!(requests[1].cache_read_tokens, None);

        // Only the first request read from the provider's cache.
        assert!(store.session_has_cache_hits(session.id).await.unwrap());

        // The context size is the latest request, not a sum; the totals are untouched.
        let usage = store.session_usage(session.id).await.unwrap();
        assert_eq!(usage.context_tokens, 1_500);
        assert_eq!(usage.input_tokens, 0);

        store.delete_session(session.id).await.unwrap();
        let requests = store.session_usage_requests(session.id).await.unwrap();
        assert!(requests.is_empty());
    }

    #[tokio::test]
    async fn identical_prompts_are_stored_once_and_go_with_their_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();
        let mut session = Session::new(dir.path());
        session.add_message(Message::user("first"));
        store.save_session(&session, "p", "m").await.unwrap();

        let a = store.record_prompt_snapshot("system", "[]").await.unwrap();
        let b = store.record_prompt_snapshot("system", "[]").await.unwrap();
        let other = store.record_prompt_snapshot("other", "[]").await.unwrap();
        assert_eq!(a, b, "the same pair is the same snapshot");
        assert_ne!(a, other);

        let latest = store.latest_prompt_snapshot(session.id).await.unwrap();
        assert_eq!(latest, None, "no request has used one yet");

        let usage = maukode_protocol::TokenUsage {
            input_tokens: 10,
            output_tokens: 1,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };
        store
            .record_usage_request(session.id, "m", &usage, Some(a.as_str()))
            .await
            .unwrap();
        let latest = store.latest_prompt_snapshot(session.id).await.unwrap();
        let latest = latest.unwrap();
        assert_eq!(latest.hash, a);
        assert_eq!(latest.system_prompt, "system");
        assert_eq!(latest.tools_json, "[]");

        let requests = store.session_usage_requests(session.id).await.unwrap();
        assert_eq!(requests[0].snapshot_hash.as_deref(), Some(a.as_str()));

        // Deleting the session drops the snapshots nothing refers to any more.
        store.delete_session(session.id).await.unwrap();
        assert_eq!(store.prompt_snapshot(&a).await.unwrap(), None);
        assert_eq!(store.prompt_snapshot(&other).await.unwrap(), None);
    }

    #[tokio::test]
    async fn save_session_appends_only_new_messages() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();
        let mut session = Session::new(dir.path());
        session.add_message(Message::user("first"));
        session.add_message(Message::assistant_text("reply"));
        store.save_session(&session, "p", "m").await.unwrap();

        let rows = |store: SqliteSessionStore, id: String| async move {
            let conn = store.conn.lock().await;
            let mut stmt = conn
                .prepare("SELECT id, created_at FROM messages WHERE session_id = ?1 ORDER BY id")
                .unwrap();
            let mapped = stmt
                .query_map(params![id], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap();
            mapped
                .collect::<rusqlite::Result<Vec<(i64, String)>>>()
                .unwrap()
        };
        let before = rows(store.clone(), session.id.to_string()).await;

        session.add_message(Message::user("second"));
        store.save_session(&session, "p", "m").await.unwrap();
        let after = rows(store.clone(), session.id.to_string()).await;

        assert_eq!(after.len(), 3);
        assert_eq!(
            after[..2],
            before[..],
            "unchanged rows must not be rewritten"
        );
        let loaded = store.load_session(session.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages[2].text_content(), "second");

        // A rewritten history (e.g. after compaction) replaces the rows from the first change.
        session.messages[1] = Message::assistant_text("edited");
        session.messages.truncate(2);
        store.save_session(&session, "p", "m").await.unwrap();
        let loaded = store.load_session(session.id).await.unwrap().unwrap();
        let texts: Vec<_> = loaded.messages.iter().map(|m| m.text_content()).collect();
        assert_eq!(texts, ["first", "edited"]);
        let rewritten = rows(store.clone(), session.id.to_string()).await;
        assert_eq!(rewritten[0], before[0]);
    }

    #[test]
    fn old_audit_table_is_rebuilt_with_content_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE messages_audit (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    parts_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(session_id, parts_json)
                );
                INSERT INTO messages_audit (session_id, role, parts_json, created_at)
                    VALUES ('s', '\"user\"', '[]', 't');",
            )
            .unwrap();
        }
        let store = SqliteSessionStore::new(&path).unwrap();
        let conn = store.conn.blocking_lock();
        let (count, hash): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), MAX(content_hash) FROM messages_audit",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(hash, content_hash("\"user\"", "[]"));
    }

    #[tokio::test]
    async fn delete_empty_sessions_removes_only_messageless_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();

        // Two empty sessions (never talked to) and one with a message.
        let empty_a = Session::new("/tmp/a");
        let empty_b = Session::new("/tmp/b");
        let mut talked = Session::new("/tmp/c");
        talked.add_message(Message::user("hello"));

        store.save_session(&empty_a, "p", "m").await.unwrap();
        store.save_session(&empty_b, "p", "m").await.unwrap();
        store.save_session(&talked, "p", "m").await.unwrap();
        assert_eq!(store.list_all_sessions().await.unwrap().len(), 3);

        let removed = store.delete_empty_sessions().await.unwrap();
        assert_eq!(removed, 2);

        let remaining = store.list_all_sessions().await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, talked.id);
        // The talked session must still load with its message intact.
        let loaded = store.load_session(talked.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 1);
    }

    #[tokio::test]
    async fn delete_empty_sessions_is_a_noop_when_all_sessions_have_messages() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::new(dir.path().join("db.sqlite")).unwrap();

        let mut s = Session::new("/tmp");
        s.add_message(Message::user("hi"));
        store.save_session(&s, "p", "m").await.unwrap();

        let removed = store.delete_empty_sessions().await.unwrap();
        assert_eq!(removed, 0);
        assert_eq!(store.list_all_sessions().await.unwrap().len(), 1);
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

    /// Many processes/tests opening a brand-new database at once used to fail with "database is
    /// locked" because the WAL switch ran before the busy timeout was installed.
    #[test]
    fn simultaneous_first_open_of_a_new_database_succeeds() {
        for round in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("fresh{round}.sqlite"));
            let barrier = Arc::new(std::sync::Barrier::new(16));
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    let (path, barrier) = (path.clone(), Arc::clone(&barrier));
                    std::thread::spawn(move || {
                        barrier.wait();
                        SqliteSessionStore::new(path).map(|_| ())
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap().expect("open failed");
            }
        }
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
