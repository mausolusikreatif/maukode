//! MauKode shell. On desktop it starts/finds the local daemon; on Android it is a plain client
//! of a remote daemon (the UI asks for its URL and token), so none of the daemon code is built.

#[cfg(desktop)]
mod desktop;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.invoke_handler(tauri::generate_handler![desktop::daemon_info]);
    builder
        .run(tauri::generate_context!())
        .expect("error while running the MauKode app");
}
