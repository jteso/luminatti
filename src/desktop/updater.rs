//! GitHub Release based updates for the unsigned macOS preview build.
//!
//! The preview channel intentionally avoids Sparkle and code signing. Releases
//! publish a zip plus a SHA-256 sidecar; the bundled helper verifies that hash,
//! swaps a user-owned app bundle after this process exits, and relaunches it.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Sender;

use serde::Deserialize;

const RELEASE_API: &str = "https://api.github.com/repos/jteso/luminatti/releases/latest";
const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/jteso/luminatti/releases/download/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Release {
    pub version: String,
    archive_url: String,
    checksum_url: String,
}

#[derive(Debug)]
pub(super) enum Event {
    Available(Release),
    Unavailable,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

pub(super) fn check_for_update(sender: Sender<Event>, current_version: &'static str) {
    std::thread::spawn(move || {
        let event = latest_release(current_version)
            .ok()
            .flatten()
            .map(Event::Available)
            .unwrap_or(Event::Unavailable);
        let _ = sender.send(event);
    });
}

fn latest_release(current_version: &str) -> Result<Option<Release>, String> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "8",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "User-Agent: Luminatti-Updater",
            RELEASE_API,
        ])
        .output()
        .map_err(|error| format!("could not start curl: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let release: GitHubRelease = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid GitHub release response: {error}"))?;
    let version = release.tag_name.trim_start_matches('v').to_string();
    if !is_newer_version(&version, current_version) {
        return Ok(None);
    }

    let archive_name = format!("Luminatti-macos-{}.zip", architecture());
    let checksum_name = format!("{archive_name}.sha256");
    let archive_url = release
        .assets
        .iter()
        .find(|asset| asset.name == archive_name)
        .map(|asset| asset.browser_download_url.clone());
    let checksum_url = release
        .assets
        .iter()
        .find(|asset| asset.name == checksum_name)
        .map(|asset| asset.browser_download_url.clone());

    match (archive_url, checksum_url) {
        (Some(archive_url), Some(checksum_url))
            if archive_url.starts_with(RELEASE_DOWNLOAD_PREFIX)
                && checksum_url.starts_with(RELEASE_DOWNLOAD_PREFIX) =>
        {
            Ok(Some(Release {
                version,
                archive_url,
                checksum_url,
            }))
        }
        _ => Ok(None),
    }
}

pub(super) fn install(release: &Release) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let app_bundle = app_bundle_from_executable(&executable).ok_or_else(|| {
        "Updates are available only from an installed Luminatti.app.".to_string()
    })?;
    let installer = app_bundle
        .join("Contents")
        .join("Resources")
        .join("install-macos-update.sh");
    if !installer.is_file() {
        return Err("The bundled update installer is missing.".to_string());
    }

    Command::new("/bin/bash")
        .arg(installer)
        .arg(&release.archive_url)
        .arg(&release.checksum_url)
        .arg(app_bundle)
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|error| format!("could not start updater: {error}"))?;
    Ok(())
}

fn app_bundle_from_executable(executable: &std::path::Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()?.to_str()? == "MacOS"
        && contents.file_name()?.to_str()? == "Contents"
        && bundle.file_name()?.to_str()? == "Luminatti.app")
        .then(|| bundle.to_path_buf())
}

fn architecture() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "arm64"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "x64"
    }
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    let parse = |version: &str| {
        let mut values = version.split('.').map(|part| part.parse::<u64>().ok());
        Some((values.next()??, values.next()??, values.next()??))
    };
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer_version;

    #[test]
    fn compares_semantic_versions_without_a_new_dependency() {
        assert!(is_newer_version("0.2.0", "0.1.9"));
        assert!(is_newer_version("1.0.0", "0.99.99"));
        assert!(!is_newer_version("0.1.9", "0.2.0"));
        assert!(!is_newer_version("not-a-version", "0.2.0"));
    }
}
