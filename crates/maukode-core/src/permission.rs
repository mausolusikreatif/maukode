use maukode_protocol::PermissionDecision;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Ask for confirmation on dangerous commands (default)
    #[default]
    Ask,
    /// Ask for confirmation on all commands and operations
    Strict,
    /// Ask for every operation and allow its result to be kept private from the model
    Review,
    /// Auto-allow all operations without prompting
    Trust,
}

impl PermissionMode {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Ask => 0,
            Self::Strict => 1,
            Self::Review => 2,
            Self::Trust => 3,
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Strict,
            2 => Self::Review,
            3 => Self::Trust,
            _ => Self::Ask,
        }
    }
}

#[derive(Clone)]
pub struct PermissionManager {
    mode: Arc<AtomicU8>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    session_whitelists: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    static_whitelist: Arc<Vec<String>>,
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new(PermissionMode::default())
    }
}

impl PermissionManager {
    pub fn new(mode: PermissionMode) -> Self {
        Self {
            mode: Arc::new(AtomicU8::new(mode.as_u8())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            session_whitelists: Arc::new(Mutex::new(HashMap::new())),
            static_whitelist: Arc::new(Vec::new()),
        }
    }

    pub fn with_whitelist(mut self, whitelist: Vec<String>) -> Self {
        self.static_whitelist = Arc::new(whitelist);
        self
    }

    /// Create a manager with its own independent mode that still shares pending requests and
    /// whitelists with `self`, so responses to prompts keep working across forks.
    pub fn fork(&self) -> Self {
        Self {
            mode: Arc::new(AtomicU8::new(self.mode.load(Ordering::Relaxed))),
            pending: Arc::clone(&self.pending),
            session_whitelists: Arc::clone(&self.session_whitelists),
            static_whitelist: Arc::clone(&self.static_whitelist),
        }
    }

    pub fn mode(&self) -> PermissionMode {
        PermissionMode::from_u8(self.mode.load(Ordering::Relaxed))
    }

    /// Change the mode for this manager and all of its clones.
    pub fn set_mode(&self, mode: PermissionMode) {
        self.mode.store(mode.as_u8(), Ordering::Relaxed);
    }

    /// Check if a command is whitelisted either statically or for this session
    pub async fn is_whitelisted(&self, session_id: &str, command: &str) -> bool {
        let trimmed = command.trim();

        // 1. Check static whitelist patterns
        for pattern in self.static_whitelist.iter() {
            if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1].trim_end();
                if trimmed.starts_with(prefix) {
                    return true;
                }
            } else if trimmed == pattern.trim() {
                return true;
            }
        }

        // 2. Check session whitelist
        let session_guard = self.session_whitelists.lock().await;
        if let Some(set) = session_guard.get(session_id) {
            if set.contains(trimmed) {
                return true;
            }
        }

        false
    }

    /// Add a command to the session whitelist
    pub async fn whitelist_session_command(&self, session_id: &str, command: &str) {
        let mut session_guard = self.session_whitelists.lock().await;
        session_guard
            .entry(session_id.to_string())
            .or_default()
            .insert(command.trim().to_string());
    }

    /// Register a pending permission request and obtain a receiver
    pub async fn register_request(
        &self,
        request_id: String,
    ) -> oneshot::Receiver<PermissionDecision> {
        let (tx, rx) = oneshot::channel();
        let mut guard = self.pending.lock().await;
        guard.insert(request_id, tx);
        rx
    }

    /// Respond to a pending permission request
    pub async fn respond(&self, request_id: &str, decision: PermissionDecision) -> bool {
        let mut guard = self.pending.lock().await;
        if let Some(tx) = guard.remove(request_id) {
            let _ = tx.send(decision);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_changes_are_shared_by_clones() {
        let manager = PermissionManager::new(PermissionMode::Ask);
        let cloned = manager.clone();

        manager.set_mode(PermissionMode::Review);

        assert_eq!(cloned.mode(), PermissionMode::Review);
    }

    #[tokio::test]
    async fn private_decision_is_delivered_to_pending_request() {
        let manager = PermissionManager::default();
        let receiver = manager.register_request("request".to_string()).await;

        assert!(
            manager
                .respond("request", PermissionDecision::AllowPrivate)
                .await
        );
        assert_eq!(receiver.await, Ok(PermissionDecision::AllowPrivate));
    }
}
