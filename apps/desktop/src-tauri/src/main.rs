// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use tokio::sync::Mutex;

/// The UI may ask for the daemon from several places at once (e.g. two windows, or React's
/// double-invoked effects in development). Only one caller at a time may look for / start it.
static DAEMON_START: Mutex<()> = Mutex::const_new(());

#[derive(Serialize)]
struct DaemonInfo {
    url: String,
    token: Option<String>,
}

/// The daemon executable: `MAUKODE_BIN`, else a `maukode` shipped next to this app (release
/// bundles), else whatever `maukode` is on `PATH`.
fn daemon_binary() -> String {
    if let Ok(bin) = std::env::var("MAUKODE_BIN") {
        return bin;
    }
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("maukode")))
        .filter(|p| p.is_file())
    {
        return sibling.to_string_lossy().into_owned();
    }
    "maukode".to_string()
}

/// Locate the MauKode daemon, starting it if needed, and hand its URL and token to the UI.
///
/// The daemon is a separate, persistent process (`maukode server start`): agents keep running
/// after the window is closed, and the CLI/TUI can attach to the same daemon. The token is
/// generated here, passed to the daemon through its environment and read back from the
/// owner-only `daemon.json`; it never appears on a command line.
#[tauri::command]
async fn daemon_info() -> Result<DaemonInfo, String> {
    let _single_flight = DAEMON_START.lock().await;
    if let Some(info) = maukode_server::get_daemon_status()
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(DaemonInfo {
            url: info.url,
            token: info.token,
        });
    }

    let bin = daemon_binary();
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let status = tokio::process::Command::new(&bin)
        .args(["server", "start"])
        .env("MAUKODE_AUTH_TOKEN", &token)
        .status()
        .await
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
