use std::path::PathBuf;
use std::time::Duration;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::client::ServerClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub url: String,
    pub pid: u32,
    pub token: Option<String>,
}

pub fn daemon_file_path() -> PathBuf {
    if let Some(mut dir) = dirs::data_dir() {
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
    if let Some(mut dir) = dirs::data_dir() {
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

pub async fn ensure_daemon_running(default_port: u16, token: Option<String>) -> Result<DaemonInfo> {
    // 1. Check if existing daemon is alive
    if let Ok(Some(info)) = get_daemon_status().await {
        return Ok(info);
    }

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
    if let Some(ref t) = token {
        cmd.arg("--token").arg(t);
    }
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

    let mut child = cmd.spawn()
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
            return Err(anyhow!("Daemon server exited immediately with status: {status}. Check log: {}", log_path.display()));
        }
    }

    if !ready {
        let _ = child.kill();
        return Err(anyhow!("Daemon server did not become healthy within 3 seconds. Check log: {}", log_path.display()));
    }

    let info = DaemonInfo {
        url,
        pid,
        token,
    };

    let daemon_file = daemon_file_path();
    if let Some(parent) = daemon_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&info)?;
    let _ = std::fs::write(&daemon_file, json);

    Ok(info)
}
