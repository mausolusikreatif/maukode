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

/// Bring the application window to the foreground and focus it.
#[tauri::command]
async fn focus_window(window: tauri::Window) -> Result<(), String> {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

/// Open a path in an external code editor (e.g. VS Code or Cursor).
#[tauri::command]
async fn open_in_editor(path: String, editor: Option<String>) -> Result<(), String> {
    let ed = editor.as_deref().unwrap_or("code");
    let bin = match ed {
        "cursor" => "cursor",
        _ => "code",
    };

    #[cfg(target_os = "windows")]
    {
        tokio::process::Command::new("cmd")
            .args(["/c", bin, &path])
            .spawn()
            .map_err(|e| format!("failed to launch {bin} for {path}: {e}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        tokio::process::Command::new(bin)
            .arg(&path)
            .spawn()
            .map_err(|e| format!("failed to launch {bin} for {path}: {e}"))?;
    }

    Ok(())
}

/// Reveal a file or folder in the operating system's native file manager.
#[tauri::command]
async fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("explorer");
        if p.is_file() {
            cmd.arg(format!("/select,{}", path));
        } else {
            cmd.arg(&path);
        }
        cmd.spawn().map_err(|e| format!("failed to reveal in Explorer: {e}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        let mut cmd = tokio::process::Command::new("open");
        if p.is_file() {
            cmd.arg("-R").arg(&path);
        } else {
            cmd.arg(&path);
        }
        cmd.spawn().map_err(|e| format!("failed to reveal in Finder: {e}"))?;
    }

    #[cfg(target_os = "linux")]
    {
        let target = if p.is_file() {
            p.parent().unwrap_or(p).to_string_lossy().to_string()
        } else {
            path
        };
        tokio::process::Command::new("xdg-open")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("failed to open file manager: {e}"))?;
    }

    Ok(())
}

/// Send a native system notification where supported.
#[tauri::command]
async fn send_notification(title: String, body: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("notify-send")
            .args(["-a", "MauKode", &title, &body])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"display notification "{}" with title "{}""#,
            body.replace('"', "\\\""),
            title.replace('"', "\\\"")
        );
        let _ = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn();
    }

    let _ = (&title, &body);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            daemon_info,
            focus_window,
            open_in_editor,
            reveal_in_file_manager,
            send_notification,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the MauKode desktop app");
}
