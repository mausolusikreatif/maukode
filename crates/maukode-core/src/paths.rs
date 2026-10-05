//! Locations of MauKode's config and data. Setting `MAUKODE_HOME` relocates both (to
//! `$MAUKODE_HOME/config` and `$MAUKODE_HOME/data`), which tests and portable installs use to
//! avoid touching the user's real directories.

use std::path::PathBuf;

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
