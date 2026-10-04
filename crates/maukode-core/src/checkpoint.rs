use anyhow::{Context, Result};
use chrono::Utc;
use maukode_protocol::{
    Checkpoint, DiffResponse, FileDiffEntry, FileDiffResponse, FileDiffStatus, UndoResponse,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
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
        let temp_idx =
            std::env::temp_dir().join(format!("maukode_idx_{}_{}", session_id, Uuid::new_v4()));

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

        let tree_hash = String::from_utf8_lossy(&write_tree_out.stdout)
            .trim()
            .to_string();

        // 3. Commit tree object
        let commit_msg = format!(
            "maukode checkpoint: session {} turn {}",
            session_id, turn_index
        );
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
            String::from_utf8_lossy(&commit_out.stdout)
                .trim()
                .to_string()
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
        let temp_idx =
            std::env::temp_dir().join(format!("maukode_cur_idx_{}_{}", session_id, Uuid::new_v4()));

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
            anyhow::bail!(
                "Failed to scan current working tree: {}",
                String::from_utf8_lossy(&add_out.stderr)
            );
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
            anyhow::bail!(
                "Failed to compute current tree hash: {}",
                String::from_utf8_lossy(&write_tree_out.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&write_tree_out.stdout)
            .trim()
            .to_string())
    }

    /// Restore a session's checkpoints from `refs/maukode/<session>/<turn>` after a restart.
    async fn ensure_loaded(&self, session_id: &str) {
        if self
            .checkpoints
            .read()
            .await
            .get(session_id)
            .is_some_and(|c| !c.is_empty())
        {
            return;
        }
        let prefix = format!("refs/maukode/{session_id}/");
        let Ok(out) = tokio::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .arg("for-each-ref")
            .arg("--format=%(refname)%09%(objectname)%09%(tree)%09%(creatordate:iso-strict)%09%(contents:subject)")
            .arg(&prefix)
            .output()
            .await
        else {
            return;
        };
        if !out.status.success() {
            return;
        }
        let mut restored: Vec<Checkpoint> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let mut f = line.splitn(5, '\t');
                let refname = f.next()?;
                let commit = f.next()?.to_string();
                let tree = f.next()?;
                let created_at = f.next()?.to_string();
                let description = f.next().unwrap_or_default().to_string();
                let turn_index = refname.strip_prefix(&prefix)?.parse().ok()?;
                // `commit-tree` failures fall back to storing the bare tree as the ref target.
                let tree_hash = if tree.is_empty() {
                    commit.clone()
                } else {
                    tree.to_string()
                };
                Some(Checkpoint {
                    id: Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    turn_index,
                    description,
                    tree_hash,
                    commit_hash: commit,
                    created_at,
                })
            })
            .collect();
        if restored.is_empty() {
            return;
        }
        restored.sort_by_key(|c| c.turn_index);
        let mut lock = self.checkpoints.write().await;
        lock.entry(session_id.to_string()).or_insert(restored);
    }

    /// `(base_tree, current_tree)` for a session, or `None` when it has no checkpoints.
    async fn diff_trees(
        &self,
        session_id: &str,
        target_turn: Option<usize>,
    ) -> Result<Option<(String, String)>> {
        self.ensure_loaded(session_id).await;
        let base_tree = {
            let lock = self.checkpoints.read().await;
            match lock.get(session_id) {
                Some(cps) if !cps.is_empty() => target_turn
                    .and_then(|t| cps.iter().find(|c| c.turn_index == t))
                    .unwrap_or(&cps[0])
                    .tree_hash
                    .clone(),
                _ => return Ok(None),
            }
        };
        let current_tree = self.current_working_tree_hash(session_id).await?;
        Ok(Some((base_tree, current_tree)))
    }

    async fn git_diff(
        &self,
        args: &[&str],
        base: &str,
        current: &str,
        path: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("--git-dir")
            .arg(&self.git_dir)
            .arg("diff")
            .args(args)
            .arg(base)
            .arg(current)
            .env("GIT_LITERAL_PATHSPECS", "1");
        if let Some(p) = path {
            cmd.arg("--").arg(p);
        }
        let out = cmd.output().await?;
        if !out.status.success() {
            anyhow::bail!("git diff failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(out.stdout)
    }

    /// Calculate diff between session baseline checkpoint and current working directory.
    pub async fn get_diff(
        &self,
        session_id: &str,
        target_turn: Option<usize>,
    ) -> Result<DiffResponse> {
        let Some((base_tree, current_tree)) = self.diff_trees(session_id, target_turn).await?
        else {
            return Ok(DiffResponse {
                session_id: session_id.to_string(),
                files_changed: 0,
                insertions: 0,
                deletions: 0,
                stat_summary: "No checkpoints recorded for this session.".to_string(),
                diff_content: String::new(),
            });
        };

        let stat = self
            .git_diff(&["--stat"], &base_tree, &current_tree, None)
            .await?;
        let stat_summary = String::from_utf8_lossy(&stat).trim().to_string();
        // e.g. "2 files changed, 5 insertions(+), 1 deletion(-)"
        let (files_changed, insertions, deletions) = parse_diff_stat(&stat_summary);

        let diff = self
            .git_diff(&["--color=never"], &base_tree, &current_tree, None)
            .await?;

        Ok(DiffResponse {
            session_id: session_id.to_string(),
            files_changed,
            insertions,
            deletions,
            stat_summary,
            diff_content: String::from_utf8_lossy(&diff).to_string(),
        })
    }

    /// Per-file change list (status and line counts) against the baseline checkpoint.
    pub async fn get_diff_files(
        &self,
        session_id: &str,
        target_turn: Option<usize>,
    ) -> Result<Vec<FileDiffEntry>> {
        let Some((base, current)) = self.diff_trees(session_id, target_turn).await? else {
            return Ok(Vec::new());
        };
        let status = self
            .git_diff(
                &["--name-status", "--no-renames", "-z"],
                &base,
                &current,
                None,
            )
            .await?;
        let numstat = self
            .git_diff(&["--numstat", "--no-renames", "-z"], &base, &current, None)
            .await?;

        // numstat -z: "<ins>\t<del>\t<path>\0" ; "-" counts mean binary.
        let mut counts: HashMap<String, (Option<usize>, Option<usize>)> = HashMap::new();
        for rec in String::from_utf8_lossy(&numstat)
            .split('\0')
            .filter(|r| !r.is_empty())
        {
            let mut f = rec.splitn(3, '\t');
            if let (Some(i), Some(d), Some(path)) = (f.next(), f.next(), f.next()) {
                counts.insert(path.to_string(), (i.parse().ok(), d.parse().ok()));
            }
        }

        // name-status -z: "<S>\0<path>\0"
        let text = String::from_utf8_lossy(&status).to_string();
        let mut parts = text.split('\0').filter(|p| !p.is_empty());
        let mut files = Vec::new();
        while let (Some(code), Some(path)) = (parts.next(), parts.next()) {
            let status = match code {
                "A" => FileDiffStatus::Added,
                "D" => FileDiffStatus::Deleted,
                _ => FileDiffStatus::Modified,
            };
            let (insertions, deletions) = counts.get(path).copied().unwrap_or((None, None));
            files.push(FileDiffEntry {
                path: path.to_string(),
                status,
                binary: insertions.is_none() && deletions.is_none(),
                insertions,
                deletions,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Unified diff for one file (literal path, no pathspec magic), capped at 1 MiB.
    pub async fn get_file_diff(
        &self,
        session_id: &str,
        path: &str,
        target_turn: Option<usize>,
    ) -> Result<FileDiffResponse> {
        const MAX_BYTES: usize = 1024 * 1024;
        if path.is_empty() || path.contains('\0') {
            anyhow::bail!("invalid path");
        }
        let Some((base, current)) = self.diff_trees(session_id, target_turn).await? else {
            anyhow::bail!("No checkpoints recorded for this session");
        };
        let raw = self
            .git_diff(
                &["--color=never", "--no-renames"],
                &base,
                &current,
                Some(path),
            )
            .await?;
        let truncated = raw.len() > MAX_BYTES;
        let slice = if truncated {
            &raw[..MAX_BYTES]
        } else {
            &raw[..]
        };
        Ok(FileDiffResponse {
            session_id: session_id.to_string(),
            path: path.to_string(),
            diff: String::from_utf8_lossy(slice).to_string(),
            truncated,
        })
    }

    /// Roll back changes to previous checkpoint (or N turns back).
    pub async fn undo(&self, session_id: &str, turns_back: Option<usize>) -> Result<UndoResponse> {
        let n_back = turns_back.unwrap_or(1).max(1);
        self.ensure_loaded(session_id).await;
        let mut lock = self.checkpoints.write().await;
        let session_cps = lock.get_mut(session_id).ok_or_else(|| {
            anyhow::anyhow!(
                "No checkpoints available to undo for session {}",
                session_id
            )
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
        let temp_idx = std::env::temp_dir().join(format!(
            "maukode_undo_idx_{}_{}",
            session_id,
            Uuid::new_v4()
        ));

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
            anyhow::bail!(
                "Failed to read target tree for undo: {}",
                String::from_utf8_lossy(&read_tree_out.stderr)
            );
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
            anyhow::bail!(
                "Failed to checkout files during undo: {}",
                String::from_utf8_lossy(&checkout_out.stderr)
            );
        }

        // Truncate checkpoints after target_idx so subsequent undos operate cleanly
        let dropped: Vec<usize> = session_cps[target_idx + 1..]
            .iter()
            .map(|c| c.turn_index)
            .collect();
        session_cps.truncate(target_idx + 1);
        // Drop the shadow refs too, or the undone checkpoints would reappear after a restart.
        for turn in dropped {
            let _ = tokio::process::Command::new("git")
                .arg("--git-dir")
                .arg(&self.git_dir)
                .arg("update-ref")
                .arg("-d")
                .arg(format!("refs/maukode/{session_id}/{turn}"))
                .output()
                .await;
        }

        Ok(UndoResponse {
            session_id: session_id.to_string(),
            target_turn,
            restored_checkpoint_id: target_cp.id,
            message: format!(
                "Reverted workspace to checkpoint before turn #{}",
                target_turn
            ),
        })
    }

    /// List all checkpoints recorded for a session
    pub async fn list_checkpoints(&self, session_id: &str) -> Vec<Checkpoint> {
        self.ensure_loaded(session_id).await;
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
            if let Some(num) = trimmed
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<usize>().ok())
            {
                files = num;
            }
        } else if trimmed.contains("insertion") {
            if let Some(num) = trimmed
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<usize>().ok())
            {
                insertions = num;
            }
        } else if trimmed.contains("deletion") {
            if let Some(num) = trimmed
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<usize>().ok())
            {
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
        let cp1 = mgr
            .create_checkpoint(session_id, 1, "Initial state")
            .await?;
        assert_eq!(cp1.turn_index, 1);

        // 2. Simulate AI modifications: modify file1 and add file2
        tokio::fs::write(&file1, "fn main() { println!(\"v2 modified\"); }").await?;
        let file2 = temp_dir.join("extra.rs");
        tokio::fs::write(&file2, "pub fn helper() {}").await?;

        // 3. Inspect diff
        let diff = mgr.get_diff(session_id, None).await?;
        assert_eq!(diff.files_changed, 2);
        assert!(diff.diff_content.contains("+pub fn helper() {}"));
        assert!(diff
            .diff_content
            .contains("-fn main() { println!(\"v1\"); }"));
        assert!(diff
            .diff_content
            .contains("+fn main() { println!(\"v2 modified\"); }"));

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

    #[tokio::test]
    async fn test_per_file_diff_and_restore_from_refs() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("maukode_ckpt_files_{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join("keep.txt"), "one\n").await?;
        tokio::fs::write(dir.join("gone.txt"), "bye\n").await?;

        let mgr = CheckpointManager::new(&dir).await?;
        mgr.create_checkpoint("sess", 0, "baseline").await?;

        tokio::fs::write(dir.join("keep.txt"), "one\ntwo\n").await?;
        tokio::fs::write(dir.join("new file.txt"), "n\n").await?;
        tokio::fs::remove_file(dir.join("gone.txt")).await?;

        let files = mgr.get_diff_files("sess", None).await?;
        let by_path = |p: &str| files.iter().find(|f| f.path == p).cloned();
        assert_eq!(
            by_path("keep.txt").unwrap().status,
            FileDiffStatus::Modified
        );
        assert_eq!(by_path("keep.txt").unwrap().insertions, Some(1));
        assert_eq!(
            by_path("new file.txt").unwrap().status,
            FileDiffStatus::Added
        );
        assert_eq!(by_path("gone.txt").unwrap().status, FileDiffStatus::Deleted);

        let one = mgr.get_file_diff("sess", "keep.txt", None).await?;
        assert!(one.diff.contains("+two") && !one.diff.contains("gone.txt"));
        // Pathspec magic is treated literally, so this matches nothing instead of everything.
        let magic = mgr.get_file_diff("sess", ":(glob)*", None).await?;
        assert!(magic.diff.is_empty());

        // A fresh manager (daemon restart) rebuilds checkpoints from shadow refs.
        let restarted = CheckpointManager::new(&dir).await?;
        assert_eq!(restarted.list_checkpoints("sess").await.len(), 1);
        assert_eq!(restarted.get_diff_files("sess", None).await?.len(), 3);

        let _ = tokio::fs::remove_dir_all(&dir).await;
        Ok(())
    }
}
