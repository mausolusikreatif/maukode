//! Opening a URL in the user's default browser (local machine only).

/// Try to open `url` with the platform's default handler. Returns whether a launcher was started.
pub fn open_url(url: &str) -> bool {
    #[cfg(target_os = "linux")]
    let command = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let command = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let command = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let command: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Browser launch is unsupported on this platform",
    ));
    command.is_ok()
}
