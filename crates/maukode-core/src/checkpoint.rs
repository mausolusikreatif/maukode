use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result};
use chrono::Utc;
use maukode_protocol::{Checkpoint, DiffResponse, UndoResponse};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CheckpointManager {
    pub workspace_root: PathBuf,
    pub git_dir: PathBuf,
    pub is_external_shadow: bool,
    checkpoints: Arc<RwLock<HashMap<String, Vec<Checkpoint>>>>,
}

impl CheckpointManager {
    pub async fn new(workspace_root: impl Into<PathBuf>) -> Result<Self> {
        let root = workspace_root.into();
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());

        // Check if workspace is part of a git repository
        let is_git = tokio::process::Command::new("git")
            .arg("rev-parse")
            .arg("--is-inside-work-tree")
            .current_dir(&canonical_root)
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        let (git_dir, is_external_shadow) = if is_git {
            let out = tokio::process::Command::new("git")
                .arg("rev-parse")
                .arg("--git-dir")
                .current_dir(&canonical_root)
                .output()
                .await?;
            let git_dir_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let p = PathBuf::from(git_dir_str);
            let resolved = if p.is_absolute() {
                p
            } else {
                canonical_root.join(p)
            };
            (resolved, false)
        } else {
            // Create a shadow git repository in ~/.local/share/maukode/shadow_git/<hash>
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            canonical_root.to_string_lossy().hash(&mut hasher);
            let dir_hash = format!("{:016x}", hasher.finish());

            let base = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("maukode")
                .join("shadow_git")
                .join(dir_hash);

            tokio::fs::create_dir_all(&base).await?;

            let git_dir = base.join(".git");
            if !git_dir.exists() {
                let _ = tokio::process::Command::new("git")
                    .arg("--git-dir")
                    .arg(&git_dir)
                    .arg("init")
                    .output()
                    .await;
            }
            (git_dir, true)
        };

        Ok(Self {
            workspace_root: canonical_root,
            git_dir,
            is_external_shadow,
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Snapshot the working directory and create a shadow git commit checkpoint.
    pub async fn create_checkpoint(
        &self,
        session_id: &str,
        turn_index: usize,
        description: impl Into<String>,
    ) -> Result<Checkpoint> {
        let desc = description.into();
        let temp_idx = std::env::temp_dir().join(format!("maukode_idx_{}_{}", session_id, Uuid::new_v4()));

        // 1. Stage working directory into isolated temporary index
        let add_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("--work-tree")
            .arg(&self.workspace_root)
            .arg("add")
            .arg("-A")
            .env("GIT_INDEX_FILE", &temp_idx)
            .output()
            .await
            .context("Failed to run git add for checkpoint")?;

        if !add_out.status.success() {
            let _ = tokio::fs::remove_file(&temp_idx).await;
            anyhow::bail!(
                "Failed to stage files for checkpoint: {}",
                String::from_utf8_lossy(&add_out.stderr)
            );
        }

        // 2. Write tree object
        let write_tree_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("write-tree")
            .env("GIT_INDEX_FILE", &temp_idx)
            .output()
            .await
            .context("Failed to write git tree")?;

        let _ = tokio::fs::remove_file(&temp_idx).await;

        if !write_tree_out.status.success() {
            anyhow::bail!(
                "Failed to write git tree: {}",
                String::from_utf8_lossy(&write_tree_out.stderr)
            );
        }

        let tree_hash = String::from_utf8_lossy(&write_tree_out.stdout).trim().to_string();

        // 3. Commit tree object
        let commit_msg = format!("maukode checkpoint: session {} turn {}", session_id, turn_index);
        let commit_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("commit-tree")
            .arg(&tree_hash)
            .arg("-m")
            .arg(&commit_msg)
            .output()
            .await?;

        let commit_hash = if commit_out.status.success() {
            String::from_utf8_lossy(&commit_out.stdout).trim().to_string()
        } else {
            tree_hash.clone()
        };

        // 4. Update shadow ref (refs/maukode/<session_id>/<turn_index>)
        let ref_name = format!("refs/maukode/{}/{}", session_id, turn_index);
        let _ = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("update-ref")
            .arg(&ref_name)
            .arg(&commit_hash)
            .output()
            .await;

        let checkpoint = Checkpoint {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            turn_index,
            description: desc,
            tree_hash,
            commit_hash,
            created_at: Utc::now().to_rfc3339(),
        };

        let mut lock = self.checkpoints.write().await;
        lock.entry(session_id.to_string())
            .or_default()
            .push(checkpoint.clone());

        Ok(checkpoint)
    }

    /// Capture current working tree and return current tree hash
    async fn current_working_tree_hash(&self, session_id: &str) -> Result<String> {
        let temp_idx = std::env::temp_dir().join(format!("maukode_cur_idx_{}_{}", session_id, Uuid::new_v4()));

        let add_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("--work-tree")
            .arg(&self.workspace_root)
            .arg("add")
            .arg("-A")
            .env("GIT_INDEX_FILE", &temp_idx)
            .output()
            .await?;

        if !add_out.status.success() {
            let _ = tokio::fs::remove_file(&temp_idx).await;
            anyhow::bail!("Failed to scan current working tree: {}", String::from_utf8_lossy(&add_out.stderr));
        }

        let write_tree_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("write-tree")
            .env("GIT_INDEX_FILE", &temp_idx)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&temp_idx).await;

        if !write_tree_out.status.success() {
            anyhow::bail!("Failed to compute current tree hash: {}", String::from_utf8_lossy(&write_tree_out.stderr));
        }

        Ok(String::from_utf8_lossy(&write_tree_out.stdout).trim().to_string())
    }

    /// Calculate diff between session baseline checkpoint and current working directory.
    pub async fn get_diff(&self, session_id: &str, target_turn: Option<usize>) -> Result<DiffResponse> {
        let lock = self.checkpoints.read().await;
        let session_cps = lock.get(session_id);

        let base_tree = match session_cps {
            Some(cps) if !cps.is_empty() => {
                if let Some(target) = target_turn {
                    cps.iter()
                        .find(|c| c.turn_index == target)
                        .map(|c| c.tree_hash.clone())
                        .unwrap_or_else(|| cps.first().unwrap().tree_hash.clone())
                } else {
                    // Default: baseline checkpoint at session start (first turn)
                    cps.first().unwrap().tree_hash.clone()
                }
            }
            _ => {
                return Ok(DiffResponse {
                    session_id: session_id.to_string(),
                    files_changed: 0,
                    insertions: 0,
                    deletions: 0,
                    stat_summary: "No checkpoints recorded for this session.".to_string(),
                    diff_content: String::new(),
                });
            }
        };
        drop(lock);

        let current_tree = self.current_working_tree_hash(session_id).await?;

        // 1. Get stat summary: git diff --stat <base_tree> <current_tree>
        let stat_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("diff")
            .arg("--stat")
            .arg(&base_tree)
            .arg(&current_tree)
            .output()
            .await?;

        let stat_summary = String::from_utf8_lossy(&stat_out.stdout).trim().to_string();

        // Parse files changed, insertions, deletions from stat summary (e.g. "2 files changed, 5 insertions(+), 1 deletion(-)")
        let (files_changed, insertions, deletions) = parse_diff_stat(&stat_summary);

        // 2. Get full unified diff: git diff --color=never <base_tree> <current_tree>
        let diff_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("diff")
            .arg("--color=never")
            .arg(&base_tree)
            .arg(&current_tree)
            .output()
            .await?;

        let diff_content = String::from_utf8_lossy(&diff_out.stdout).to_string();

        Ok(DiffResponse {
            session_id: session_id.to_string(),
            files_changed,
            insertions,
            deletions,
            stat_summary,
            diff_content,
        })
    }

    /// Roll back changes to previous checkpoint (or N turns back).
    pub async fn undo(&self, session_id: &str, turns_back: Option<usize>) -> Result<UndoResponse> {
        let n_back = turns_back.unwrap_or(1).max(1);
        let mut lock = self.checkpoints.write().await;
        let session_cps = lock.get_mut(session_id).ok_or_else(|| {
            anyhow::anyhow!("No checkpoints available to undo for session {}", session_id)
        })?;

        if session_cps.is_empty() {
            anyhow::bail!("Checkpoint history is empty for session {}", session_id);
        }

        // Determine target checkpoint index
        let total_cps = session_cps.len();
        let target_idx = if total_cps <= n_back {
            0
        } else {
            total_cps.saturating_sub(n_back)
        };

        let target_cp = session_cps[target_idx].clone();
        let target_tree = target_cp.tree_hash.clone();
        let target_turn = target_cp.turn_index;

        // Current tree hash before rollback
        let current_tree = self.current_working_tree_hash(session_id).await?;

        // 1. Remove files newly added since target checkpoint
        let added_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("diff")
            .arg("--name-only")
            .arg("--diff-filter=A")
            .arg(&target_tree)
            .arg(&current_tree)
            .output()
            .await?;

        if added_out.status.success() {
            let added_files = String::from_utf8_lossy(&added_out.stdout);
            for file_rel in added_files.lines() {
                let trimmed = file_rel.trim();
                if !trimmed.is_empty() {
                    let file_path = self.workspace_root.join(trimmed);
                    let _ = tokio::fs::remove_file(&file_path).await;
                }
            }
        }

        // 2. Checkout files from target tree
        let temp_idx = std::env::temp_dir().join(format!("maukode_undo_idx_{}_{}", session_id, Uuid::new_v4()));

        let read_tree_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("read-tree")
            .arg(&target_tree)
            .env("GIT_INDEX_FILE", &temp_idx)
            .output()
            .await?;

        if !read_tree_out.status.success() {
            let _ = tokio::fs::remove_file(&temp_idx).await;
            anyhow::bail!("Failed to read target tree for undo: {}", String::from_utf8_lossy(&read_tree_out.stderr));
        }

        let checkout_out = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("--work-tree")
            .arg(&self.workspace_root)
            .arg("checkout-index")
            .arg("-a")
            .arg("-f")
            .env("GIT_INDEX_FILE", &temp_idx)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&temp_idx).await;

        if !checkout_out.status.success() {
            anyhow::bail!("Failed to checkout files during undo: {}", String::from_utf8_lossy(&checkout_out.stderr));
        }

        // Truncate checkpoints after target_idx so subsequent undos operate cleanly
        session_cps.truncate(target_idx + 1);

        Ok(UndoResponse {
            session_id: session_id.to_string(),
            target_turn,
            restored_checkpoint_id: target_cp.id,
            message: format!("Reverted workspace to checkpoint before turn #{}", target_turn),
        })
    }

    /// List all checkpoints recorded for a session
    pub async fn list_checkpoints(&self, session_id: &str) -> Vec<Checkpoint> {
        let lock = self.checkpoints.read().await;
        lock.get(session_id).cloned().unwrap_or_default()
    }
}

fn parse_diff_stat(stat: &str) -> (usize, usize, usize) {
    let last_line = stat.lines().last().unwrap_or("");
    let mut files = 0;
    let mut insertions = 0;
    let mut deletions = 0;

    for part in last_line.split(',') {
        let trimmed = part.trim();
        if trimmed.contains("file") {
            if let Some(num) = trimmed.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()) {
                files = num;
            }
        } else if trimmed.contains("insertion") {
            if let Some(num) = trimmed.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()) {
                insertions = num;
            }
        } else if trimmed.contains("deletion") {
            if let Some(num) = trimmed.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()) {
                deletions = num;
            }
        }
    }

    (files, insertions, deletions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_checkpoint_lifecycle_and_undo() -> Result<()> {
        let temp_dir = std::env::temp_dir().join(format!("maukode_ckpt_test_{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&temp_dir).await?;

        // Create initial file
        let file1 = temp_dir.join("main.rs");
        tokio::fs::write(&file1, "fn main() { println!(\"v1\"); }").await?;

        let mgr = CheckpointManager::new(&temp_dir).await?;
        let session_id = "test-sess-1";

        // 1. Create baseline checkpoint
        let cp1 = mgr.create_checkpoint(session_id, 1, "Initial state").await?;
        assert_eq!(cp1.turn_index, 1);

        // 2. Simulate AI modifications: modify file1 and add file2
        tokio::fs::write(&file1, "fn main() { println!(\"v2 modified\"); }").await?;
        let file2 = temp_dir.join("extra.rs");
        tokio::fs::write(&file2, "pub fn helper() {}").await?;

        // 3. Inspect diff
        let diff = mgr.get_diff(session_id, None).await?;
        assert_eq!(diff.files_changed, 2);
        assert!(diff.diff_content.contains("+pub fn helper() {}"));
        assert!(diff.diff_content.contains("-fn main() { println!(\"v1\"); }"));
        assert!(diff.diff_content.contains("+fn main() { println!(\"v2 modified\"); }"));

        // 4. Undo modifications
        let undo_res = mgr.undo(session_id, Some(1)).await?;
        assert_eq!(undo_res.target_turn, 1);

        // Verify file1 restored to v1 and file2 deleted
        let content1 = tokio::fs::read_to_string(&file1).await?;
        assert_eq!(content1, "fn main() { println!(\"v1\"); }");
        assert!(!file2.exists(), "extra.rs should have been deleted by undo");

        // Verify diff is now clean
        let clean_diff = mgr.get_diff(session_id, None).await?;
        assert_eq!(clean_diff.files_changed, 0);

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        Ok(())
    }
}
