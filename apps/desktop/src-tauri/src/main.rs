// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;

#[derive(Serialize)]
struct DaemonInfo {
    url: String,
    token: Option<String>,
}

/// Locate the MauKode daemon, starting it if needed, and hand its URL and token to the UI.
///
/// The daemon is a separate, persistent process (`maukode server start`): agents keep running
/// after the window is closed, and the CLI/TUI can attach to the same daemon. The token is
/// generated here, passed to the daemon through its environment and read back from the
/// owner-only `daemon.json`; it never appears on a command line.
#[tauri::command]
async fn daemon_info() -> Result<DaemonInfo, String> {
    if let Some(info) = maukode_server::get_daemon_status()
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(DaemonInfo {
            url: info.url,
            token: info.token,
        });
    }

    let bin = std::env::var("MAUKODE_BIN").unwrap_or_else(|_| "maukode".to_string());
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let status = std::process::Command::new(&bin)
        .args(["server", "start"])
        .env("MAUKODE_AUTH_TOKEN", &token)
        .status()
        .map_err(|e| format!("could not run `{bin}` (set MAUKODE_BIN): {e}"))?;
    if !status.success() {
        return Err(format!("`{bin} server start` failed with {status}"));
    }

    match maukode_server::get_daemon_status()
        .await
        .map_err(|e| e.to_string())?
    {
        Some(info) => Ok(DaemonInfo {
            url: info.url,
            token: info.token,
        }),
        None => Err("the daemon started but did not register itself".to_string()),
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![daemon_info])
        .run(tauri::generate_context!())
        .expect("error while running the MauKode desktop app");
}
