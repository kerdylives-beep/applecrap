use std::{env, fs, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use semver::Version;
use serde::Deserialize;

use crate::models::UpdateInfo;

const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/kerdylives-beep/applecrap/releases/latest";
const UPDATER_USER_AGENT: &str = "AppleCrap-Alpha-Updater";

#[derive(Deserialize)]
struct ReleasePayload {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub fn parse_version_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches('v')).ok()
}

/// Ask GitHub for the latest release; Some(info) only when it is strictly
/// newer than the running version and carries a portable zip asset.
pub async fn check_latest(client: &reqwest::Client, current: &str) -> Result<Option<UpdateInfo>> {
    let current = parse_version_tag(current)
        .ok_or_else(|| anyhow!("The running version \"{current}\" is not valid semver."))?;

    let payload: ReleasePayload = client
        .get(RELEASES_LATEST_URL)
        .header("User-Agent", UPDATER_USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("GitHub latest-release request failed")?
        .json()
        .await?;

    let Some(latest) = parse_version_tag(&payload.tag_name) else {
        return Ok(None);
    };
    if latest <= current {
        return Ok(None);
    }

    let Some(asset) = payload
        .assets
        .iter()
        .find(|asset| asset.name.to_lowercase().ends_with(".zip"))
    else {
        return Ok(None);
    };

    Ok(Some(UpdateInfo {
        version: payload.tag_name,
        release_url: payload.html_url,
        asset_url: asset.browser_download_url.clone(),
    }))
}

/// Download the release zip and stage the new executable next to the current
/// one as "<name>.new". Settings are untouched: the update replaces only the
/// exe, and all app data lives in ./data beside it.
pub async fn download_and_stage(client: &reqwest::Client, asset_url: &str) -> Result<PathBuf> {
    let bytes = client
        .get(asset_url)
        .header("User-Agent", UPDATER_USER_AGENT)
        .send()
        .await?
        .error_for_status()
        .context("Update download failed")?
        .bytes()
        .await?;

    let staged = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf> {
        let reader = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).context("The update zip is unreadable")?;
        let exe_index = (0..archive.len())
            .find(|&index| {
                archive
                    .by_index(index)
                    .map(|entry| entry.name().to_lowercase().ends_with(".exe"))
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow!("The update zip does not contain an executable."))?;

        let mut entry = archive.by_index(exe_index)?;
        let current_exe = env::current_exe()?;
        let staged = current_exe.with_extension("new");
        let mut output = fs::File::create(&staged)
            .with_context(|| format!("Cannot write next to the app at {}", staged.display()))?;
        std::io::copy(&mut entry, &mut output)?;
        Ok(staged)
    })
    .await??;

    Ok(staged)
}

/// Swap the staged executable into place and launch it. The running image is
/// renamed aside (allowed on Windows), the new exe takes its path, and the
/// stale ".old" is cleaned up on the next start.
pub fn swap_and_launch(staged: &PathBuf) -> Result<()> {
    let current_exe = env::current_exe()?;
    let old = current_exe.with_extension("old");

    let _ = fs::remove_file(&old);
    fs::rename(&current_exe, &old)
        .context("Could not move the running executable aside. Is the app folder writable?")?;

    if let Err(error) = fs::rename(staged, &current_exe) {
        // Roll back so the app still launches from its original path.
        let _ = fs::rename(&old, &current_exe);
        return Err(anyhow::Error::new(error).context("Could not move the new executable into place"));
    }

    std::process::Command::new(&current_exe)
        .spawn()
        .context("The update installed but the new version failed to launch. Start it manually.")?;
    Ok(())
}

/// Remove leftovers from a previous update. Deleting the ".old" image fails
/// while the old process is still exiting; that is fine — it succeeds on a
/// later start.
pub fn clean_stale_artifacts() {
    if let Ok(current_exe) = env::current_exe() {
        let _ = fs::remove_file(current_exe.with_extension("old"));
        let _ = fs::remove_file(current_exe.with_extension("new"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_tags() {
        assert_eq!(
            parse_version_tag("v0.3.0-alpha.1"),
            Some(Version::parse("0.3.0-alpha.1").unwrap())
        );
        assert_eq!(
            parse_version_tag("1.2.3"),
            Some(Version::parse("1.2.3").unwrap())
        );
        assert_eq!(parse_version_tag("not-a-version"), None);
    }

    #[test]
    fn orders_prerelease_versions_correctly() {
        let alpha1 = parse_version_tag("v0.3.0-alpha.1").unwrap();
        let alpha2 = parse_version_tag("v0.3.0-alpha.2").unwrap();
        let stable = parse_version_tag("v0.3.0").unwrap();
        let next = parse_version_tag("v0.4.0-alpha.1").unwrap();

        assert!(alpha1 < alpha2);
        assert!(alpha2 < stable);
        assert!(stable < next);
    }
}
