use crate::client::ServerClient;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub url: String,
    pub pid: u32,
    pub token: Option<String>,
}

pub fn daemon_file_path() -> PathBuf {
    if let Some(mut dir) = maukode_core::paths::data_dir() {
        dir.push("maukode");
        dir.push("daemon.json");
        dir
    } else if let Some(mut dir) = dirs::home_dir() {
        dir.push(".maukode");
        dir.push("daemon.json");
        dir
    } else {
        PathBuf::from(".maukode_daemon.json")
    }
}

pub fn daemon_log_path() -> PathBuf {
    if let Some(mut dir) = maukode_core::paths::data_dir() {
        dir.push("maukode");
        dir.push("server.log");
        dir
    } else if let Some(mut dir) = dirs::home_dir() {
        dir.push(".maukode");
        dir.push("server.log");
        dir
    } else {
        PathBuf::from(".maukode_server.log")
    }
}

pub async fn check_daemon_alive(url: &str, token: Option<&str>) -> bool {
    let client = match ServerClient::new(url, token.map(|s| s.to_string())) {
        Ok(c) => c,
        Err(_) => return false,
    };

    tokio::time::timeout(Duration::from_millis(800), client.health())
        .await
        .unwrap_or(Ok(false))
        .unwrap_or(false)
}

pub async fn get_daemon_status() -> Result<Option<DaemonInfo>> {
    let path = daemon_file_path();
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)?;
    let info: DaemonInfo = serde_json::from_str(&content)?;

    if check_daemon_alive(&info.url, info.token.as_deref()).await {
        Ok(Some(info))
    } else {
        // Clean stale daemon.json
        let _ = std::fs::remove_file(&path);
        Ok(None)
    }
}

pub async fn stop_daemon() -> Result<bool> {
    let path = daemon_file_path();
    if !path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&path)?;
    let info: DaemonInfo = serde_json::from_str(&content)?;

    // Terminate process
    #[cfg(unix)]
    {
        let pid = info.pid as i32;
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .output();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &info.pid.to_string(), "/F"])
            .output();
    }

    let _ = std::fs::remove_file(&path);
    Ok(true)
}

/// Held while starting a daemon so concurrent starters (GUI + CLI, or two UI windows) cannot both
/// spawn one: the loser would otherwise register its own token for a daemon that is not running.
struct StartLock(#[allow(dead_code)] std::fs::File);

async fn acquire_start_lock() -> Result<StartLock> {
    let path = daemon_file_path().with_file_name("daemon.lock");
    tokio::task::spawn_blocking(move || -> Result<StartLock> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // Blocks until the current holder finishes; released when the file is closed.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(StartLock(file))
    })
    .await?
}

/// True if `url` accepts `token` (health alone is public, so it proves nothing about auth).
async fn accepts_token(url: &str, token: Option<&str>) -> bool {
    match ServerClient::new(url, token.map(str::to_string)) {
        Ok(client) => matches!(
            tokio::time::timeout(Duration::from_millis(800), client.list_providers()).await,
            Ok(Ok(_))
        ),
        Err(_) => false,
    }
}

pub async fn ensure_daemon_running(default_port: u16, token: Option<String>) -> Result<DaemonInfo> {
    // 1. Check if existing daemon is alive
    if let Ok(Some(info)) = get_daemon_status().await {
        return Ok(info);
    }

    // Serialize starters, then look again: whoever held the lock may have just started one.
    let _lock = acquire_start_lock().await?;
    if let Ok(Some(info)) = get_daemon_status().await {
        return Ok(info);
    }

    // Never start a daemon without a token: on loopback, any local user or process could
    // otherwise drive the agent. The generated token is saved in the owner-only daemon file,
    // where local clients pick it up.
    let token = token
        .filter(|t| !t.trim().is_empty())
        .or_else(|| Some(generate_token()));

    // 2. Resolve current executable
    let current_exe = std::env::current_exe()?;
    let bind_addr = format!("127.0.0.1:{default_port}");
    let url = format!("http://127.0.0.1:{default_port}");

    let log_path = daemon_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let mut cmd = std::process::Command::new(&current_exe);
    cmd.arg("server").arg("--bind").arg(&bind_addr);
    // Via the environment, not argv: command lines are readable by every local user (`ps`).
    match token {
        Some(ref t) => cmd.env("MAUKODE_AUTH_TOKEN", t),
        None => cmd.env_remove("MAUKODE_AUTH_TOKEN"),
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file.try_clone()?))
        .stderr(std::process::Stdio::from(log_file));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn background daemon server: {e}"))?;

    let pid = child.id();

    // 3. Wait for daemon to become healthy (up to 3 seconds)
    let mut ready = false;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if check_daemon_alive(&url, token.as_deref()).await {
            ready = true;
            break;
        }

        // Check if process crashed immediately
        if let Ok(Some(status)) = child.try_wait() {
            return Err(anyhow!(
                "Daemon server exited immediately with status: {status}. Check log: {}",
                log_path.display()
            ));
        }
    }

    if !ready {
        let _ = child.kill();
        return Err(anyhow!(
            "Daemon server did not become healthy within 3 seconds. Check log: {}",
            log_path.display()
        ));
    }

    // Something answered /health, but is it *our* daemon? A stale daemon on the port with a
    // different token would pass the health check while rejecting the token we are about to save.
    if !accepts_token(&url, token.as_deref()).await || matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
        return Err(anyhow!(
            "Port {default_port} is already served by another MauKode daemon that does not accept \
             this token (or the new daemon exited). Stop it with `maukode server stop` or pick \
             another port. Log: {}",
            log_path.display()
        ));
    }

    let info = DaemonInfo { url, pid, token };

    let daemon_file = daemon_file_path();
    if let Some(parent) = daemon_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&info)?;
    write_private(&daemon_file, &json)?;

    Ok(info)
}

/// A random bearer token (two v4 UUIDs: 244 random bits).
fn generate_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Browser address of the daemon's web UI, with the token in the fragment (never sent to the
/// server in a request line, so it does not end up in access logs).
pub fn web_ui_url(info: &DaemonInfo) -> String {
    match &info.token {
        Some(token) => format!("{}#token={token}", info.url),
        None => info.url.clone(),
    }
}

/// Write a file only the current user can read: it holds the daemon's bearer token.
fn write_private(path: &std::path::Path, contents: &str) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // `mode` only applies on creation; tighten a pre-existing file as well.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn start_lock_serializes_concurrent_starters() {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("MAUKODE_HOME", home.path());
        let first = acquire_start_lock().await.unwrap();
        let second = tokio::spawn(acquire_start_lock());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !second.is_finished(),
            "second starter must wait for the first"
        );
        drop(first);
        let waited = tokio::time::timeout(Duration::from_secs(5), second).await;
        assert!(
            waited.is_ok(),
            "second starter proceeds once the lock is released"
        );
    }

    #[test]
    fn daemon_file_is_private_even_when_it_already_existed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.json");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, "{\"token\":\"t\"}").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"token\":\"t\"}");
    }
}
