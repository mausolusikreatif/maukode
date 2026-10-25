//! `maukode update`: replace an `install.sh` installation with a newer GitHub release.
//!
//! The release tarball is the same one `install.sh` downloads: its checksum is verified, the new
//! web UI is swapped in first, and the binary is replaced last with a rename. Replacing a running
//! executable by rename is safe on Linux, so the old process keeps working until it is restarted.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO: &str = "mausolusikreatif/maukode";

/// How often the "update available" check asks GitHub.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Set to a value other than `0` to turn the "update available" check off.
const NO_CHECK_ENV: &str = "MAUKODE_NO_UPDATE_CHECK";

/// Where `install.sh` put things: `<prefix>/bin/maukode` and `<prefix>/share/maukode/web`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallLayout {
    pub binary: PathBuf,
    pub web_dir: PathBuf,
}

impl InstallLayout {
    /// The installation `exe` belongs to, or `None` when it was not put there by `install.sh`
    /// (for example `cargo install`, which has no web UI directory next to it).
    pub fn of(exe: &Path) -> Option<Self> {
        let bin_dir = exe.parent()?;
        if bin_dir.file_name()? != "bin" {
            return None;
        }
        let prefix = bin_dir.parent()?;
        let web_dir = prefix.join("share").join("maukode").join("web");
        if !web_dir.join("index.html").is_file() {
            return None;
        }
        Some(Self {
            binary: exe.to_path_buf(),
            web_dir,
        })
    }
}

/// `v1.2.3` or `1.2.3` as numbers; `None` for anything else.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Whether release `tag` is newer than the running `current` version.
pub fn is_newer(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(tag), Some(current)) => tag > current,
        _ => false,
    }
}

/// `0.5.0` and `v0.5.0` both become the release tag `v0.5.0`.
pub fn normalize_tag(text: &str) -> Result<String> {
    if parse_version(text).is_none() {
        bail!("'{text}' is not a version like v0.5.0");
    }
    Ok(format!("v{}", text.trim().trim_start_matches('v')))
}

/// The checksum in a `sha256sum` file: the first word of its first line.
fn expected_sha256(contents: &str) -> Option<&str> {
    contents.lines().next()?.split_whitespace().next()
}

pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("maukode/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("could not create the HTTP client")
}

/// The tag of the latest release. `/releases/latest` redirects to `/releases/tag/<tag>`, which
/// needs no API token and has no rate limit to run into.
pub async fn latest_tag(client: &reqwest::Client) -> Result<String> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let response = client
        .head(&url)
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?;
    let response = response.error_for_status()?;
    let path = response.url().path().to_string();
    let tag = path.rsplit('/').next().unwrap_or_default();
    if parse_version(tag).is_none() {
        bail!("could not read the latest release from {url}");
    }
    Ok(tag.to_string())
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not download {url}"))?;
    let response = response.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

/// Download release `tag`, verify it, and install it over `layout`. Nothing is replaced unless
/// the checksum matches and the archive has the expected layout.
pub async fn install(layout: &InstallLayout, tag: &str) -> Result<()> {
    let name = format!("maukode-linux-x86_64-{tag}");
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let client = http_client()?;

    let archive = download(&client, &format!("{base}/{name}.tar.gz")).await?;
    let sums = download(&client, &format!("{base}/{name}.tar.gz.sha256")).await?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = expected_sha256(&sums)
        .ok_or_else(|| anyhow!("the checksum file for {tag} is malformed"))?;
    let actual = format!("{:x}", Sha256::digest(&archive));
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("the download of {tag} does not match its checksum; nothing was changed");
    }

    // Beside the web directory, so the final renames stay on one filesystem.
    let staging = layout
        .web_dir
        .with_file_name(format!(".update-{}", std::process::id()));
    std::fs::create_dir_all(&staging).context("could not create a staging directory")?;
    let result = apply(layout, &archive, &name, &staging).await;
    let _ = std::fs::remove_dir_all(&staging);
    result
}

async fn apply(layout: &InstallLayout, archive: &[u8], name: &str, staging: &Path) -> Result<()> {
    let archive_path = staging.join("release.tar.gz");
    std::fs::write(&archive_path, archive).context("could not save the download")?;
    let status = tokio::process::Command::new("tar")
        .arg("xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(staging)
        .status()
        .await
        .context("could not run tar")?;
    if !status.success() {
        bail!("tar could not unpack the release; nothing was changed");
    }

    let package = staging.join(name);
    let new_binary = package.join("maukode");
    let new_web = package.join("web");
    if !new_binary.is_file() || !new_web.join("index.html").is_file() {
        bail!("the release archive has an unexpected layout; nothing was changed");
    }

    swap_dir(&new_web, &layout.web_dir)?;

    let bin_dir = layout
        .binary
        .parent()
        .ok_or_else(|| anyhow!("the binary has no parent directory"))?;
    let pending = bin_dir.join(".maukode.new");
    std::fs::copy(&new_binary, &pending).context("could not copy the new binary")?;
    std::fs::rename(&pending, &layout.binary).context("could not replace the binary")?;
    Ok(())
}

/// Put `new` in place of `target`, restoring the old directory if the move fails.
fn swap_dir(new: &Path, target: &Path) -> Result<()> {
    let old = target.with_file_name("web.old");
    let _ = std::fs::remove_dir_all(&old);
    let had_old = target.exists();
    if had_old {
        std::fs::rename(target, &old).context("could not move the old web UI aside")?;
    }
    if let Err(error) = std::fs::rename(new, target) {
        if had_old {
            let _ = std::fs::rename(&old, target);
        }
        let error = anyhow::Error::new(error);
        return Err(error.context("could not put the new web UI in place"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// What the last "update available" check found, kept in `<data dir>/maukode/update-check.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CheckCache {
    /// Unix time of the check, in seconds.
    checked_at: u64,
    /// The latest release tag then; empty when the check could not reach GitHub.
    latest: String,
}

fn now_secs() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_secs(),
        Err(_) => 0,
    }
}

fn check_path() -> Option<PathBuf> {
    let dir = crate::paths::data_dir()?;
    Some(dir.join("maukode").join("update-check.json"))
}

fn read_check(path: &Path) -> Option<CheckCache> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn is_stale(cache: Option<&CheckCache>, now: u64) -> bool {
    match cache {
        Some(cache) => now.saturating_sub(cache.checked_at) >= CHECK_INTERVAL_SECS,
        None => true,
    }
}

fn notice_for(cache: &CheckCache, current: &str) -> Option<String> {
    if !is_newer(&cache.latest, current) {
        return None;
    }
    Some(format!(
        "MauKode {} is available (this is {current}). Run `maukode update`.",
        cache.latest
    ))
}

/// Only an `install.sh` installation can run `maukode update`, so only it is told about updates
/// (and only it asks GitHub). `MAUKODE_NO_UPDATE_CHECK=1` turns the check off.
fn check_allowed() -> bool {
    let opted_out = match std::env::var_os(NO_CHECK_ENV) {
        Some(value) => !value.is_empty() && value != "0",
        None => false,
    };
    if opted_out {
        return false;
    }
    let exe = std::env::current_exe().ok();
    exe.and_then(|exe| InstallLayout::of(&exe)).is_some()
}

/// Ask GitHub for the latest release and store the answer. When GitHub cannot be reached the
/// time is still stored, so an offline machine asks once a day and not at every start.
async fn refresh_check(path: PathBuf, previous: Option<CheckCache>) {
    let latest = match http_client() {
        Ok(client) => {
            let wait = Duration::from_secs(10);
            let asked = tokio::time::timeout(wait, latest_tag(&client)).await;
            asked.ok().and_then(|result| result.ok())
        }
        Err(_) => None,
    };
    let latest = latest
        .or_else(|| previous.map(|cache| cache.latest))
        .unwrap_or_default();
    let cache = CheckCache {
        checked_at: now_secs(),
        latest,
    };
    let Ok(json) = serde_json::to_string(&cache) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, json);
}

/// For the start of an interactive command: the line to show when an earlier check found a newer
/// release than `current`. A check older than a day is refreshed in the background, so its result
/// shows up on a later start and nothing waits for the network. Must run inside a tokio runtime.
pub fn startup_notice(current: &str) -> Option<String> {
    if !check_allowed() {
        return None;
    }
    let path = check_path()?;
    let cache = read_check(&path);
    if is_stale(cache.as_ref(), now_secs()) {
        tokio::spawn(refresh_check(path, cache.clone()));
    }
    notice_for(&cache?, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_with_or_without_the_v() {
        assert_eq!(parse_version("v0.5.0"), Some((0, 5, 0)));
        assert_eq!(parse_version("1.12.3"), Some((1, 12, 3)));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    fn newer_means_a_higher_number_not_a_longer_string() {
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("v0.5.0", "0.5.0"));
        assert!(!is_newer("v0.4.0", "0.5.0"));
        assert!(!is_newer("nope", "0.5.0"));
    }

    #[test]
    fn a_tag_is_normalized_or_rejected() {
        assert_eq!(normalize_tag("0.5.0").unwrap(), "v0.5.0");
        assert_eq!(normalize_tag("v0.5.0").unwrap(), "v0.5.0");
        assert!(normalize_tag("main").is_err());
    }

    fn cache(checked_at: u64, latest: &str) -> CheckCache {
        CheckCache {
            checked_at,
            latest: latest.to_string(),
        }
    }

    #[test]
    fn a_check_goes_stale_after_a_day() {
        let day = CHECK_INTERVAL_SECS;
        assert!(is_stale(None, 1_000));
        assert!(!is_stale(Some(&cache(1_000, "v0.6.0")), 1_000 + day - 1));
        assert!(is_stale(Some(&cache(1_000, "v0.6.0")), 1_000 + day));
        // The clock went back: not stale.
        assert!(!is_stale(Some(&cache(5_000, "v0.6.0")), 1_000));
    }

    #[test]
    fn the_notice_names_a_newer_release_only() {
        let newer = notice_for(&cache(0, "v0.7.0"), "0.6.0").unwrap();
        assert!(newer.contains("v0.7.0"));
        assert!(newer.contains("maukode update"));
        assert_eq!(notice_for(&cache(0, "v0.6.0"), "0.6.0"), None);
        assert_eq!(notice_for(&cache(0, "v0.5.0"), "0.6.0"), None);
        // A check that could not reach GitHub stores no tag.
        assert_eq!(notice_for(&cache(0, ""), "0.6.0"), None);
    }

    #[test]
    fn a_stored_check_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        assert_eq!(read_check(&path), None);
        const STORED: &str = r#"{"checked_at":7,"latest":"v0.7.0"}"#;
        std::fs::write(&path, STORED).unwrap();
        assert_eq!(read_check(&path), Some(cache(7, "v0.7.0")));
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(read_check(&path), None);
    }

    #[test]
    fn the_checksum_is_the_first_word() {
        assert_eq!(expected_sha256("abc123  maukode.tar.gz\n"), Some("abc123"));
        assert_eq!(expected_sha256(""), None);
    }

    #[test]
    fn only_an_install_sh_layout_is_recognized() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let web = dir.path().join("share").join("maukode").join("web");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("maukode");
        assert_eq!(InstallLayout::of(&exe), None, "no web UI next to it");

        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("index.html"), "<html>").unwrap();
        let layout = InstallLayout::of(&exe).unwrap();
        assert_eq!(layout.web_dir, web);

        let elsewhere = dir.path().join("sbin").join("maukode");
        assert_eq!(
            InstallLayout::of(&elsewhere),
            None,
            "not in a bin directory"
        );
    }

    #[test]
    fn swapping_replaces_the_directory_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("web");
        let new = dir.path().join("incoming");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("old.txt"), "old").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("new.txt"), "new").unwrap();

        swap_dir(&new, &target).unwrap();

        assert!(target.join("new.txt").is_file());
        assert!(!target.join("old.txt").exists());
        assert!(!dir.path().join("web.old").exists());
        assert!(!new.exists());
    }
}
