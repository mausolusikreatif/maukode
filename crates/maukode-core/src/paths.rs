//! Locations of MauKode's config and data. Setting `MAUKODE_HOME` relocates both (to
//! `$MAUKODE_HOME/config` and `$MAUKODE_HOME/data`), which tests and portable installs use to
//! avoid touching the user's real directories.

use std::path::{Path, PathBuf};

fn home_override() -> Option<PathBuf> {
    std::env::var_os("MAUKODE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn data_dir() -> Option<PathBuf> {
    home_override()
        .map(|h| h.join("data"))
        .or_else(dirs::data_dir)
}

pub fn config_dir() -> Option<PathBuf> {
    home_override()
        .map(|h| h.join("config"))
        .or_else(dirs::config_dir)
}

/// Where chat-only sessions keep their working directories: one empty folder per session.
pub fn chats_dir() -> Option<PathBuf> {
    let dir = data_dir()?;
    Some(dir.join("maukode").join("chats"))
}

/// Whether `dir` is a chat-only session's working directory.
pub fn is_chat_dir(dir: &Path) -> bool {
    match chats_dir() {
        Some(root) => is_inside(dir, &root),
        None => false,
    }
}

fn is_inside(dir: &Path, root: &Path) -> bool {
    dir.starts_with(root) && dir != root
}

/// A session id as it is used for a directory name: hex digits and dashes only.
fn is_session_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Create the working directory of chat session `id`.
pub fn create_chat_dir(id: &str) -> std::io::Result<PathBuf> {
    let Some(root) = chats_dir() else {
        return Err(std::io::Error::other("there is no data directory"));
    };
    if !is_session_id(id) {
        return Err(std::io::Error::other("not a session id"));
    }
    let dir = root.join(id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Remove the working directory of chat session `id`, if it has one.
pub fn remove_chat_dir(id: &str) {
    if let (Some(root), true) = (chats_dir(), is_session_id(id)) {
        let _ = std::fs::remove_dir_all(root.join(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_folder_inside_the_chats_directory_is_a_chat_dir() {
        let root = Path::new("/data/maukode/chats");
        assert!(is_inside(&root.join("abc"), root));
        assert!(is_inside(&root.join("abc").join("sub"), root));
        assert!(!is_inside(root, root));
        assert!(!is_inside(Path::new("/data/maukode/chats-other/abc"), root));
        assert!(!is_inside(Path::new("/home/me/project"), root));
    }

    #[test]
    fn a_session_id_cannot_climb_out_of_the_chats_directory() {
        assert!(is_session_id("d47349eb-a621-49b7-a3dd-69bca3a3517a"));
        assert!(!is_session_id(""));
        assert!(!is_session_id(".."));
        assert!(!is_session_id("../x"));
        assert!(!is_session_id("a/b"));
    }
}
