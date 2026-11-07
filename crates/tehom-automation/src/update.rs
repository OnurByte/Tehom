//! Updates: find the latest GitHub release, install it with that release's
//! own installer (which verifies SHA-256), and roll back to the previous
//! binaries kept by the installer.
//!
//! Only three fixed addresses are contacted: the GitHub releases API for this
//! repository, the installer script pinned to the release tag, and (inside the
//! installer) the release download. Nothing is installed without a request.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPOSITORY: &str = "OnurByte/Tehom";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Folder next to the binaries where installers keep the replaced version.
pub const PREVIOUS_DIR: &str = ".tehom-previous";

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    /// Tag such as `v0.2.1`.
    pub tag: String,
    pub version: (u64, u64, u64),
    pub url: String,
    /// The first part of the release notes.
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Release,
    pub available: bool,
}

/// Parses `v1.2.3` or `1.2.3` (a pre-release suffix is ignored).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Tehom/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

fn releases_url() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases/latest")
}

/// The installer script as published with `tag`, so an update never runs
/// newer, unreleased installer code.
pub fn installer_url(tag: &str) -> Result<String> {
    ensure!(parse_version(tag).is_some(), "Invalid release tag");
    let name = if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    };
    Ok(format!(
        "https://raw.githubusercontent.com/{REPOSITORY}/{tag}/{name}"
    ))
}

/// Interprets the GitHub "latest release" JSON.
pub fn parse_release(value: &serde_json::Value) -> Result<Release> {
    let tag = value["tag_name"]
        .as_str()
        .context("The release has no tag")?
        .to_owned();
    let version = parse_version(&tag).context("The release tag is not a version")?;
    let url = value["html_url"].as_str().unwrap_or_default().to_owned();
    ensure!(
        url.is_empty() || url.starts_with(&format!("https://github.com/{REPOSITORY}/")),
        "Unexpected release address"
    );
    let notes: String = value["body"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(600)
        .collect();
    Ok(Release {
        tag,
        version,
        url,
        notes,
    })
}

/// Asks GitHub for the latest release and compares it with this build.
pub async fn check() -> Result<UpdateStatus> {
    let response = client()?
        .get(releases_url())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Could not reach GitHub to check for updates")?;
    ensure!(
        response.status().is_success(),
        "GitHub answered {} while checking for updates",
        response.status()
    );
    let bytes = response.bytes().await?;
    ensure!(
        bytes.len() <= 2 * 1024 * 1024,
        "Release information is too large"
    );
    let latest = parse_release(&serde_json::from_slice(&bytes)?)?;
    let current = parse_version(CURRENT_VERSION).unwrap_or((0, 0, 0));
    Ok(UpdateStatus {
        current: CURRENT_VERSION.to_owned(),
        available: latest.version > current,
        latest,
    })
}

/// Installs `tag` into `bin_dir` by running that release's installer, which
/// downloads the archive, verifies its SHA-256, keeps the replaced binaries
/// in `.tehom-previous` and never touches user data.
pub async fn install(tag: &str, bin_dir: &Path) -> Result<()> {
    let url = installer_url(tag)?;
    let response = client()?
        .get(&url)
        .send()
        .await
        .context("Could not download the installer")?;
    ensure!(
        response.status().is_success(),
        "GitHub answered {} for the installer",
        response.status()
    );
    let script = response.bytes().await?;
    ensure!(
        script.len() <= 256 * 1024,
        "The installer is unexpectedly large"
    );
    let folder = tempfile::tempdir()?;
    let path = folder.path().join(if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    });
    tokio::fs::write(&path, &script).await?;
    let mut command = if cfg!(windows) {
        let mut command = tokio::process::Command::new("powershell");
        command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&path);
        command
    } else {
        let mut command = tokio::process::Command::new("sh");
        command.arg(&path);
        command
    };
    let status = command
        .env("TEHOM_VERSION", tag)
        .env("TEHOM_BIN_DIR", bin_dir)
        .env("TEHOM_NO_DEPS", "1")
        .status()
        .await
        .context("The installer could not start")?;
    ensure!(status.success(), "The installer failed ({status})");
    Ok(())
}

/// The folder holding the running `tehom` binaries.
pub fn binary_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    Ok(exe
        .parent()
        .context("The binary has no folder")?
        .to_path_buf())
}

/// Restores the binaries kept in `bin_dir/.tehom-previous`. A running binary
/// is renamed aside first, which works on Windows too.
pub fn rollback(bin_dir: &Path) -> Result<Vec<String>> {
    let previous = bin_dir.join(PREVIOUS_DIR);
    let mut restored = Vec::new();
    let entries = std::fs::read_dir(&previous)
        .context("No previous version is kept; nothing to roll back")?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("tehom") || !entry.file_type()?.is_file() {
            continue;
        }
        let target = bin_dir.join(&name);
        let aside = bin_dir.join(format!(".{name}.old"));
        let _ = std::fs::remove_file(&aside);
        if target.exists() {
            std::fs::rename(&target, &aside)?;
        }
        if let Err(error) = std::fs::copy(entry.path(), &target) {
            // Put the current binary back if the copy failed.
            let _ = std::fs::rename(&aside, &target);
            return Err(error.into());
        }
        restored.push(name);
    }
    if restored.is_empty() {
        bail!("No previous version is kept; nothing to roll back");
    }
    restored.sort();
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.2.1"), Some((0, 2, 1)));
        assert_eq!(parse_version("1.10.0-beta.1"), Some((1, 10, 0)));
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        for bad in ["", "v1", "1.2", "1.2.3.4", "latest", "v1.x.0"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(parse_version(CURRENT_VERSION).is_some());
    }

    #[test]
    fn releases_are_parsed_and_pinned_to_this_repository() {
        let release = parse_release(&serde_json::json!({
            "tag_name": "v9.8.7",
            "html_url": "https://github.com/OnurByte/Tehom/releases/tag/v9.8.7",
            "body": "Notes"
        }))
        .unwrap();
        assert_eq!(release.version, (9, 8, 7));
        assert!(
            parse_release(&serde_json::json!({
                "tag_name": "v9.8.7",
                "html_url": "https://evil.example/Tehom"
            }))
            .is_err()
        );
        assert!(parse_release(&serde_json::json!({"tag_name": "nightly"})).is_err());
        let url = installer_url("v9.8.7").unwrap();
        assert!(
            url.starts_with("https://raw.githubusercontent.com/OnurByte/Tehom/v9.8.7/install.")
        );
        assert!(installer_url("main").is_err() && installer_url("../x").is_err());
    }

    #[test]
    fn rollback_restores_previous_binaries_and_keeps_the_current_aside() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path();
        std::fs::write(bin.join("tehom"), b"new").unwrap();
        assert!(rollback(bin).is_err(), "nothing kept yet");
        std::fs::create_dir(bin.join(PREVIOUS_DIR)).unwrap();
        std::fs::write(bin.join(PREVIOUS_DIR).join("tehom"), b"old").unwrap();
        std::fs::write(bin.join(PREVIOUS_DIR).join("tehom-desktop"), b"old ui").unwrap();
        std::fs::write(bin.join(PREVIOUS_DIR).join("notes.txt"), b"ignored").unwrap();
        assert_eq!(rollback(bin).unwrap(), ["tehom", "tehom-desktop"]);
        assert_eq!(std::fs::read(bin.join("tehom")).unwrap(), b"old");
        assert_eq!(std::fs::read(bin.join("tehom-desktop")).unwrap(), b"old ui");
        assert_eq!(std::fs::read(bin.join(".tehom.old")).unwrap(), b"new");
        assert!(!bin.join("notes.txt").exists());
    }
}
