//! Updates: find the latest GitHub release, download its archive and verify
//! the published SHA-256 before anything changes, then let that release's own
//! installer place it (desktop entry, app bundle, bundled tools) from the
//! verified file. Every file the archive carries ends up beside the programs;
//! if any step fails, every replaced file is put back.
//!
//! Only GitHub is contacted: the releases API for this repository, the
//! installer script pinned to the release tag, and the release download
//! (github.com redirecting to *.githubusercontent.com). Nothing is installed
//! without a request.
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

/// The release archive for this system, as the installers name it.
pub fn archive_name() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "tehom-linux-x86_64.tar.gz",
        ("linux", "aarch64") => "tehom-linux-aarch64.tar.gz",
        ("macos", "x86_64") => "tehom-macos-x86_64.tar.gz",
        ("macos", "aarch64") => "tehom-macos-aarch64.tar.gz",
        ("windows", "x86_64") => "tehom-windows-x86_64.zip",
        _ => bail!("There is no Tehom release for this system"),
    })
}

fn asset_url(tag: &str, name: &str) -> Result<String> {
    ensure!(parse_version(tag).is_some(), "Invalid release tag");
    Ok(format!(
        "https://github.com/{REPOSITORY}/releases/download/{tag}/{name}"
    ))
}

/// Release downloads start at github.com and redirect to GitHub's asset
/// hosts; no other host, scheme, port or credential is followed.
fn release_host(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == "github.com" || host.ends_with(".githubusercontent.com"))
}

/// Streams a release file to `destination` (at most `limit` bytes),
/// following redirects by hand. Returns its SHA-256 in hex.
async fn download_to(url: &str, destination: &Path, limit: u64) -> Result<String> {
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    let mut url = reqwest::Url::parse(url)?;
    let client = client()?;
    let mut response = None;
    for _ in 0..5 {
        ensure!(release_host(&url), "Update download refused: {url}");
        let reply = client
            .get(url.clone())
            .timeout(Duration::from_secs(3600))
            .send()
            .await
            .context("Could not download the update")?;
        if reply.status().is_redirection() {
            let location = reply
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .context("The download redirected without a location")?;
            url = url.join(location)?;
            continue;
        }
        ensure!(
            reply.status().is_success(),
            "GitHub answered {} for the update",
            reply.status()
        );
        response = Some(reply);
        break;
    }
    let mut response = response.context("The update download redirected too often")?;
    let mut file = tokio::fs::File::create(destination).await?;
    let mut hash = sha2::Sha256::new();
    let mut written = 0u64;
    while let Some(chunk) = response.chunk().await? {
        written += chunk.len() as u64;
        ensure!(written <= limit, "The update is unexpectedly large");
        hash.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Compares a SHA-256 in hex with the first word of a `.sha256` file.
pub fn verify_checksum(actual: &str, published: &str) -> Result<()> {
    let expected = published.split_whitespace().next().unwrap_or("");
    ensure!(
        expected.len() == 64 && expected.eq_ignore_ascii_case(actual),
        "The update's SHA-256 does not match the published one; nothing was installed"
    );
    Ok(())
}

/// Downloads `tag`, verifies it and installs it into `bin_dir`. Returns the
/// names of the files the release carries.
pub async fn install(tag: &str, bin_dir: &Path) -> Result<Vec<String>> {
    let name = archive_name()?;
    let work = tempfile::tempdir()?;
    let archive = work.path().join(name);
    let digest = download_to(&asset_url(tag, name)?, &archive, 2 * 1024 * 1024 * 1024).await?;
    let sum_path = work.path().join(format!("{name}.sha256"));
    download_to(&asset_url(tag, &format!("{name}.sha256"))?, &sum_path, 4096).await?;
    verify_checksum(&digest, &tokio::fs::read_to_string(&sum_path).await?)?;
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
    let path = work.path().join(if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    });
    tokio::fs::write(&path, &script).await?;
    install_archive(&archive, &path, tag, bin_dir).await
}

/// Files replaced during an install, kept so a failure can put them back.
struct Backup {
    bin_dir: PathBuf,
    folder: PathBuf,
    /// (file name, whether it existed before).
    files: Vec<(String, bool)>,
}

impl Backup {
    fn take(bin_dir: &Path, folder: PathBuf, names: &[String]) -> Result<Self> {
        std::fs::create_dir_all(&folder)?;
        let mut files = Vec::new();
        for name in names {
            let target = bin_dir.join(name);
            let existed = target.is_file();
            if existed {
                std::fs::copy(&target, folder.join(name))?;
            }
            files.push((name.clone(), existed));
        }
        Ok(Self {
            bin_dir: bin_dir.to_path_buf(),
            folder,
            files,
        })
    }

    /// Puts every file back as it was. A running program cannot be
    /// overwritten on Windows but can be renamed aside, so that is done first.
    fn restore(&self) -> Result<()> {
        for (name, existed) in &self.files {
            let target = self.bin_dir.join(name);
            let aside = self.bin_dir.join(format!(".{name}.failed"));
            let _ = std::fs::remove_file(&aside);
            if target.exists() && std::fs::remove_file(&target).is_err() {
                std::fs::rename(&target, &aside)?;
            }
            if *existed {
                std::fs::copy(self.folder.join(name), &target)?;
            }
        }
        Ok(())
    }

    /// Keeps the replaced files for `tehom update --rollback`.
    fn keep_as_previous(&self) -> Result<()> {
        let previous = self.bin_dir.join(PREVIOUS_DIR);
        std::fs::create_dir_all(&previous)?;
        for (name, existed) in &self.files {
            if *existed {
                std::fs::copy(self.folder.join(name), previous.join(name))?;
            }
        }
        Ok(())
    }
}

/// Installs an archive whose SHA-256 was verified (`<archive>.sha256` lies
/// beside it) with the release's installer, then places any file of the
/// archive the installer did not. On any failure every file is put back.
pub async fn install_archive(
    archive: &Path,
    installer: &Path,
    tag: &str,
    bin_dir: &Path,
) -> Result<Vec<String>> {
    let work = archive.parent().context("The archive has no folder")?;
    let unpacked = work.join("unpacked");
    tokio::fs::create_dir_all(&unpacked).await?;
    // tar reads .tar.gz everywhere and .zip on Windows 10 and later.
    let status = tokio::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(&unpacked)
        .status()
        .await
        .context("The update could not be unpacked")?;
    ensure!(status.success(), "The update could not be unpacked");
    let package = if unpacked.join("tehom").is_dir() {
        unpacked.join("tehom")
    } else {
        unpacked.clone()
    };
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&package)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    let program = format!("tehom{}", std::env::consts::EXE_SUFFIX);
    ensure!(
        names.contains(&program),
        "The update archive does not contain {program}"
    );
    std::fs::create_dir_all(bin_dir)?;
    let backup = Backup::take(bin_dir, work.join("backup"), &names)?;
    let result = async {
        let mut command = if cfg!(windows) {
            let mut command = tokio::process::Command::new("powershell");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(installer);
            command
        } else {
            let mut command = tokio::process::Command::new("sh");
            command.arg(installer);
            command
        };
        let status = command
            .env("TEHOM_VERSION", tag)
            .env("TEHOM_BIN_DIR", bin_dir)
            .env("TEHOM_ARCHIVE", archive)
            .env("TEHOM_NO_DEPS", "1")
            .status()
            .await
            .context("The installer could not start")?;
        ensure!(status.success(), "The installer failed ({status})");
        // Files a release added that this installer does not know about.
        for name in &names {
            let target = bin_dir.join(name);
            if !target.exists() {
                let partial = bin_dir.join(format!(".{name}.new"));
                std::fs::copy(package.join(name), &partial)?;
                std::fs::rename(&partial, &target)?;
            }
        }
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = result {
        backup
            .restore()
            .context("The update failed and the previous version could not be fully put back; run `tehom update --rollback`")?;
        return Err(error.context("The update failed; the previous version was put back"));
    }
    backup.keep_as_previous()?;
    Ok(names)
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
        if !(name.starts_with("tehom") || name.starts_with("whisper"))
            || !entry.file_type()?.is_file()
        {
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

    #[test]
    fn downloads_stay_on_github_and_checksums_must_match() {
        for allowed in [
            "https://github.com/OnurByte/Tehom/releases/download/v1.0.0/x.tar.gz",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1",
            "https://objects.githubusercontent.com/x",
        ] {
            assert!(
                release_host(&reqwest::Url::parse(allowed).unwrap()),
                "{allowed}"
            );
        }
        for refused in [
            "http://github.com/x",
            "https://github.com:8443/x",
            "https://u:p@github.com/x",
            "https://githubusercontent.com.evil.example/x",
            "https://evil.example/x",
        ] {
            assert!(
                !release_host(&reqwest::Url::parse(refused).unwrap()),
                "{refused}"
            );
        }
        let sum = "a".repeat(64);
        assert!(verify_checksum(&sum, &format!("{}  tehom.tar.gz\n", sum.to_uppercase())).is_ok());
        assert!(verify_checksum(&sum, &format!("{}  tehom.tar.gz", "b".repeat(64))).is_err());
        assert!(verify_checksum(&sum, "").is_err());
        assert!(asset_url("main", "x").is_err());
        assert!(archive_name().is_ok());
    }

    /// A release archive with an extra bundled tool, and an installer that
    /// knows only the two programs (or fails half-way).
    #[cfg(unix)]
    fn release_fixture(dir: &Path, installer_fails: bool) -> (PathBuf, PathBuf) {
        let package = dir.join("pkg/tehom");
        std::fs::create_dir_all(&package).unwrap();
        for (name, body) in [
            ("tehom", "new cli"),
            ("tehom-desktop", "new app"),
            ("whisper-cli", "speech"),
            ("org.tehom.Tehom.svg", "<svg/>"),
        ] {
            std::fs::write(package.join(name), body).unwrap();
        }
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let archive = work.join("tehom-test.tar.gz");
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(dir.join("pkg"))
                .arg("tehom")
                .status()
                .unwrap()
                .success()
        );
        let installer = work.join("install.sh");
        std::fs::write(
            &installer,
            format!(
                "set -eu\ntmp=$(mktemp -d)\ntar -xzf \"$TEHOM_ARCHIVE\" -C \"$tmp\"\ncp \"$tmp/tehom/tehom\" \"$TEHOM_BIN_DIR/tehom\"\n{}cp \"$tmp/tehom/tehom-desktop\" \"$TEHOM_BIN_DIR/tehom-desktop\"\n",
                if installer_fails { "exit 3\n" } else { "" }
            ),
        )
        .unwrap();
        (archive, installer)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn every_archive_file_is_installed_and_kept_for_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, installer) = release_fixture(dir.path(), false);
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("tehom"), "old cli").unwrap();
        std::fs::write(bin.join("tehom-desktop"), "old app").unwrap();
        let names = install_archive(&archive, &installer, "v9.9.9", &bin)
            .await
            .unwrap();
        assert_eq!(
            names,
            [
                "org.tehom.Tehom.svg",
                "tehom",
                "tehom-desktop",
                "whisper-cli"
            ]
        );
        assert_eq!(
            std::fs::read_to_string(bin.join("tehom")).unwrap(),
            "new cli"
        );
        assert_eq!(
            std::fs::read_to_string(bin.join("whisper-cli")).unwrap(),
            "speech"
        );
        assert!(bin.join("org.tehom.Tehom.svg").is_file());
        assert_eq!(
            std::fs::read_to_string(bin.join(PREVIOUS_DIR).join("tehom-desktop")).unwrap(),
            "old app"
        );
        rollback(&bin).unwrap();
        assert_eq!(
            std::fs::read_to_string(bin.join("tehom")).unwrap(),
            "old cli"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_install_puts_every_file_back() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, installer) = release_fixture(dir.path(), true);
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("tehom"), "old cli").unwrap();
        std::fs::write(bin.join("tehom-desktop"), "old app").unwrap();
        let error = install_archive(&archive, &installer, "v9.9.9", &bin)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("put back"), "{error:#}");
        assert_eq!(
            std::fs::read_to_string(bin.join("tehom")).unwrap(),
            "old cli"
        );
        assert_eq!(
            std::fs::read_to_string(bin.join("tehom-desktop")).unwrap(),
            "old app"
        );
        assert!(!bin.join("whisper-cli").exists());
        assert!(!bin.join(PREVIOUS_DIR).exists());
    }
}
