use std::{env, fs, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::Deserialize;

use crate::models::UpdateInfo;

const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/kerdylives-beep/applecrap/releases/latest";
const UPDATER_USER_AGENT: &str = "AppleCrap-Alpha-Updater";

/// Public half of the ed25519 key that signs releases. The private half
/// lives outside the repository. An update is installed only if its zip
/// verifies against this key, so a compromised GitHub account alone cannot
/// push code onto every install.
const RELEASE_PUBLIC_KEY: [u8; 32] = [
    0x9b, 0x3f, 0x4a, 0xd5, 0x8c, 0x3e, 0xbc, 0x25, 0xa5, 0x33, 0xf2, 0x23, 0xe7, 0xb8, 0xc8,
    0xa4, 0x91, 0xa5, 0x96, 0x19, 0x98, 0xa4, 0x0d, 0x90, 0xb0, 0x55, 0x19, 0x62, 0x73, 0x1d,
    0x1a, 0xcb,
];

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
    let signature = payload
        .assets
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(&format!("{}.sig", asset.name)));

    Ok(Some(UpdateInfo {
        version: payload.tag_name,
        release_url: payload.html_url,
        asset_url: asset.browser_download_url.clone(),
        signature_url: signature.map(|sig| sig.browser_download_url.clone()),
    }))
}

/// Download the release zip, verify its signature, and stage the new
/// executable next to the current one as "<name>.new". Nothing is extracted
/// from a zip that fails verification. Settings are untouched: the update
/// replaces only the exe, and all app data lives in ./data beside it.
pub async fn download_and_stage(client: &reqwest::Client, update: &UpdateInfo) -> Result<PathBuf> {
    let Some(signature_url) = update.signature_url.as_deref() else {
        anyhow::bail!("this release isn't signed, so it can't be installed automatically");
    };

    let signature = client
        .get(signature_url)
        .header("User-Agent", UPDATER_USER_AGENT)
        .send()
        .await?
        .error_for_status()
        .context("Update signature download failed")?
        .text()
        .await?;

    let bytes = client
        .get(&update.asset_url)
        .header("User-Agent", UPDATER_USER_AGENT)
        // A multi-megabyte download needs far longer than the shared
        // client's default, which is sized for small API calls.
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()
        .context("Update download failed")?
        .bytes()
        .await?;

    verify_release(&bytes, &signature)
        .context("the update failed its signature check and was not installed")?;

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

/// Checks a release zip against its detached, base64-encoded ed25519
/// signature (produced by `scripts/pack-portable.mjs`).
fn verify_release(zip: &[u8], signature_b64: &str) -> Result<()> {
    let key = VerifyingKey::from_bytes(&RELEASE_PUBLIC_KEY)
        .map_err(|_| anyhow!("embedded release key is invalid"))?;
    verify_with_key(&key, zip, signature_b64)
}

fn verify_with_key(key: &VerifyingKey, data: &[u8], signature_b64: &str) -> Result<()> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(signature_b64.trim())
        .map_err(|_| anyhow!("signature is not valid base64"))?;
    let signature =
        Signature::from_slice(&raw).map_err(|_| anyhow!("signature has the wrong length"))?;
    key.verify_strict(data, &signature)
        .map_err(|_| anyhow!("signature does not match the release"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // Produced by Node's crypto.sign() with the real release key, the same
    // way pack-portable.mjs signs releases: proves the two sides agree.
    const SELFTEST_MESSAGE: &[u8] = b"applecrap-release-signing-selftest";
    const SELFTEST_SIGNATURE: &str =
        "IIy3FdysZcB0Oz2WZuox8naP1b4NPA0pr1ovOmyCK0pHx505jNeChRiKermWzCUN1cGP2rvuVzHYPq+4M3JqBQ==";

    #[test]
    fn node_signature_verifies_against_embedded_key() {
        verify_release(SELFTEST_MESSAGE, SELFTEST_SIGNATURE).expect("release key round trip");
    }

    #[test]
    fn tampered_release_is_rejected() {
        assert!(verify_release(b"applecrap-release-signing-selftesT", SELFTEST_SIGNATURE).is_err());
    }

    #[test]
    fn signature_from_another_key_is_rejected() {
        let impostor = SigningKey::from_bytes(&[7_u8; 32]);
        let forged = impostor.sign(SELFTEST_MESSAGE);
        let forged_b64 = base64::engine::general_purpose::STANDARD.encode(forged.to_bytes());
        assert!(verify_release(SELFTEST_MESSAGE, &forged_b64).is_err());
        // ...but the same signature does verify against the impostor's key,
        // so the rejection above is about the key, not the encoding.
        assert!(verify_with_key(&impostor.verifying_key(), SELFTEST_MESSAGE, &forged_b64).is_ok());
    }

    #[test]
    fn garbage_signature_is_an_error() {
        assert!(verify_release(SELFTEST_MESSAGE, "not base64!!").is_err());
        assert!(verify_release(SELFTEST_MESSAGE, "c2hvcnQ=").is_err());
    }

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
