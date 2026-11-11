//! Finding the programs MauKode starts by name: `bash` for the `bash` tool and hooks, and
//! launchers such as `npm` or `npx`.
//!
//! On Unix this is the plain name, resolved through `PATH` by the OS. On Windows two things go
//! wrong with the plain name: `npm` and `npx` are `.cmd` files, which `CreateProcess` does not
//! find without the extension, and a `bash.exe` in `System32` is the WSL launcher, which would
//! run commands inside a Linux VM with paths that do not match the project. So Windows looks the
//! program up with `PATHEXT` and prefers Git for Windows' bash.

use std::ffi::OsString;
use std::path::PathBuf;

/// The program to run for `name`: an absolute path when it can be resolved, else `name` itself.
pub fn resolve_program(name: &str) -> OsString {
    if name.contains(['/', '\\']) {
        return name.into();
    }
    if cfg!(windows) && name == "bash" {
        if let Some(bash) = windows_bash() {
            return bash.into_os_string();
        }
    }
    which::which(name)
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|_| name.into())
}

/// Git for Windows' `bash.exe`: `MAUKODE_BASH`, the usual install locations, then any `bash` on
/// `PATH` that is not the WSL launcher in `System32`.
fn windows_bash() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MAUKODE_BASH").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    for var in [
        "ProgramFiles",
        "ProgramW6432",
        "ProgramFiles(x86)",
        "LocalAppData",
    ] {
        let Some(root) = std::env::var_os(var) else {
            continue;
        };
        let root = PathBuf::from(root);
        let git = if var == "LocalAppData" {
            root.join("Programs").join("Git")
        } else {
            root.join("Git")
        };
        let bash = git.join("bin").join("bash.exe");
        if bash.is_file() {
            return Some(bash);
        }
    }
    which::which_all("bash").ok()?.find(|p| {
        !p.to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\system32\\")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_returned_unchanged() {
        assert_eq!(
            resolve_program("/usr/bin/env"),
            OsString::from("/usr/bin/env")
        );
        assert_eq!(resolve_program(".\\tool"), OsString::from(".\\tool"));
    }

    #[test]
    fn an_unknown_program_keeps_its_name() {
        assert_eq!(
            resolve_program("maukode-no-such-program"),
            OsString::from("maukode-no-such-program")
        );
    }
}
