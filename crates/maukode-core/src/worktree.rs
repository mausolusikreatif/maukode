//! Per-session git worktrees so parallel agents never write into the same checkout.
//!
//! A worktree session starts from the project's current `HEAD` on a dedicated branch
//! (`maukode/<id8>`). Uncommitted changes in the project are *not* carried over. Results are
//! reviewed through the session diff and folded back with [`merge`].

use anyhow::{anyhow, Context, Result};
use maukode_protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::sync::Mutex;

/// Merges touch the user's checkout; run them one at a time.
static MERGE_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeInfo {
    /// Repository the worktree was created from (its top-level directory).
    pub project_dir: PathBuf,
    /// The worktree checkout; this is also the session's working directory.
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeStatus {
    #[serde(flatten)]
    pub info: WorktreeInfo,
    /// Uncommitted changes (including untracked files) in the worktree.
    pub dirty: bool,
    /// Commits on the branch beyond the base commit.
    pub commits_ahead: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MergeOutcome {
    Merged { commit: String },
    NothingToMerge,
    Conflict { files: Vec<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("'{0}' is not inside a git repository")]
    NotARepository(PathBuf),
    #[error("the repository has no commits yet; commit once before using worktree isolation")]
    NoCommits,
    #[error(
        "the project checkout has uncommitted changes to tracked files; commit or stash them first"
    )]
    DirtyProject,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    git_env(dir, args, &[]).await
}

async fn git_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .await
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn worktrees_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("maukode")
        .join("worktrees")
}

/// Create an isolated worktree for `session_id` from the repository containing `dir`.
pub async fn create(dir: &Path, session_id: SessionId) -> Result<WorktreeInfo, WorktreeError> {
    let toplevel = git(dir, &["rev-parse", "--show-toplevel"])
        .await
        .map_err(|_| WorktreeError::NotARepository(dir.to_path_buf()))?;
    let project_dir = PathBuf::from(toplevel);
    let base_commit = git(&project_dir, &["rev-parse", "--verify", "HEAD"])
        .await
        .map_err(|_| WorktreeError::NoCommits)?;

    let id = session_id.to_string();
    let branch = format!("maukode/{}", &id[..8]);
    let path = worktrees_root().join(&id);
    tokio::fs::create_dir_all(worktrees_root())
        .await
        .map_err(anyhow::Error::from)?;

    git(
        &project_dir,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path.to_string_lossy(),
            "HEAD",
        ],
    )
    .await?;

    Ok(WorktreeInfo {
        project_dir,
        path,
        branch,
        base_commit,
    })
}

pub async fn status(info: &WorktreeInfo) -> Result<WorktreeStatus> {
    let porcelain = git(&info.path, &["status", "--porcelain"]).await?;
    let range = format!("{}..HEAD", info.base_commit);
    let ahead = git(&info.path, &["rev-list", "--count", &range]).await?;
    Ok(WorktreeStatus {
        info: info.clone(),
        dirty: !porcelain.is_empty(),
        commits_ahead: ahead.parse().unwrap_or(0),
    })
}

/// Commit everything in the worktree. Returns the new commit, or `None` if there was nothing.
pub async fn commit_all(info: &WorktreeInfo, message: &str) -> Result<Option<String>> {
    git(&info.path, &["add", "-A"]).await?;
    if git(&info.path, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return Ok(None);
    }
    // Fall back to a neutral identity only when the user has none configured.
    let has_identity = git(&info.path, &["config", "user.email"])
        .await
        .is_ok_and(|v| !v.is_empty());
    let env: &[(&str, &str)] = if has_identity {
        &[]
    } else {
        &[
            ("GIT_AUTHOR_NAME", "maukode"),
            ("GIT_AUTHOR_EMAIL", "maukode@localhost"),
            ("GIT_COMMITTER_NAME", "maukode"),
            ("GIT_COMMITTER_EMAIL", "maukode@localhost"),
        ]
    };
    git_env(&info.path, &["commit", "-m", message], env).await?;
    Ok(Some(git(&info.path, &["rev-parse", "HEAD"]).await?))
}

/// Commit pending work, then merge the branch into whatever the project currently has checked
/// out. Refuses to run if tracked files in the project are modified; aborts cleanly on conflict.
pub async fn merge(info: &WorktreeInfo, message: &str) -> Result<MergeOutcome, WorktreeError> {
    let _guard = MERGE_LOCK.lock().await;

    let dirty = git(
        &info.project_dir,
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .await?;
    if !dirty.is_empty() {
        return Err(WorktreeError::DirtyProject);
    }

    commit_all(info, message).await?;
    let range = format!("{}..{}", info.base_commit, info.branch);
    let ahead = git(&info.project_dir, &["rev-list", "--count", &range]).await?;
    if ahead == "0" {
        return Ok(MergeOutcome::NothingToMerge);
    }

    let env = [
        ("GIT_AUTHOR_NAME", "maukode"),
        ("GIT_AUTHOR_EMAIL", "maukode@localhost"),
        ("GIT_COMMITTER_NAME", "maukode"),
        ("GIT_COMMITTER_EMAIL", "maukode@localhost"),
    ];
    let has_identity = git(&info.project_dir, &["config", "user.email"])
        .await
        .is_ok_and(|v| !v.is_empty());
    let env: &[(&str, &str)] = if has_identity { &[] } else { &env };

    match git_env(
        &info.project_dir,
        &["merge", "--no-ff", "-m", message, &info.branch],
        env,
    )
    .await
    {
        Ok(_) => Ok(MergeOutcome::Merged {
            commit: git(&info.project_dir, &["rev-parse", "HEAD"]).await?,
        }),
        Err(_) => {
            let files = git(
                &info.project_dir,
                &["diff", "--name-only", "--diff-filter=U"],
            )
            .await
            .unwrap_or_default();
            let files: Vec<String> = files.lines().map(str::to_string).collect();
            let _ = git(&info.project_dir, &["merge", "--abort"]).await;
            if files.is_empty() {
                Err(anyhow!("git merge failed").into())
            } else {
                Ok(MergeOutcome::Conflict { files })
            }
        }
    }
}

/// Remove the worktree checkout and (optionally) its branch.
pub async fn remove(info: &WorktreeInfo, delete_branch: bool) -> Result<()> {
    let path = info.path.to_string_lossy().to_string();
    if git(&info.project_dir, &["worktree", "remove", "--force", &path])
        .await
        .is_err()
    {
        let _ = tokio::fs::remove_dir_all(&info.path).await;
        let _ = git(&info.project_dir, &["worktree", "prune"]).await;
    }
    if delete_branch {
        let _ = git(&info.project_dir, &["branch", "-D", &info.branch]).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_repo(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]).await.unwrap();
        git(dir, &["config", "user.email", "t@t"]).await.unwrap();
        git(dir, &["config", "user.name", "t"]).await.unwrap();
        tokio::fs::write(dir.join("a.txt"), "one\n").await.unwrap();
        git(dir, &["add", "-A"]).await.unwrap();
        git(dir, &["commit", "-q", "-m", "init"]).await.unwrap();
    }

    #[tokio::test]
    async fn worktree_lifecycle_merge_and_conflict() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path()).await;

        // Two isolated agents.
        let a = create(repo.path(), SessionId::new()).await.unwrap();
        let b = create(repo.path(), SessionId::new()).await.unwrap();
        assert_ne!(a.path, b.path);

        tokio::fs::write(a.path.join("a.txt"), "from a\n")
            .await
            .unwrap();
        tokio::fs::write(b.path.join("a.txt"), "from b\n")
            .await
            .unwrap();
        tokio::fs::write(b.path.join("new.txt"), "b\n")
            .await
            .unwrap();
        // The project checkout is untouched by either agent.
        assert_eq!(
            tokio::fs::read_to_string(repo.path().join("a.txt"))
                .await
                .unwrap(),
            "one\n"
        );
        assert!(status(&a).await.unwrap().dirty);

        assert!(matches!(
            merge(&a, "merge a").await.unwrap(),
            MergeOutcome::Merged { .. }
        ));
        assert_eq!(
            tokio::fs::read_to_string(repo.path().join("a.txt"))
                .await
                .unwrap(),
            "from a\n"
        );

        // b conflicts on a.txt; merge aborts and the project stays clean.
        match merge(&b, "merge b").await.unwrap() {
            MergeOutcome::Conflict { files } => assert_eq!(files, vec!["a.txt".to_string()]),
            other => panic!("expected conflict, got {other:?}"),
        }
        assert!(git(repo.path(), &["status", "--porcelain"])
            .await
            .unwrap()
            .is_empty());

        // Dirty project blocks merges.
        tokio::fs::write(repo.path().join("a.txt"), "dirty\n")
            .await
            .unwrap();
        assert!(matches!(
            merge(&b, "x").await,
            Err(WorktreeError::DirtyProject)
        ));

        remove(&a, true).await.unwrap();
        remove(&b, true).await.unwrap();
        assert!(!a.path.exists() && !b.path.exists());
    }

    #[tokio::test]
    async fn rejects_non_repo_and_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            create(dir.path(), SessionId::new()).await,
            Err(WorktreeError::NotARepository(_))
        ));
        git(dir.path(), &["init", "-q"]).await.unwrap();
        assert!(matches!(
            create(dir.path(), SessionId::new()).await,
            Err(WorktreeError::NoCommits)
        ));
    }
}
