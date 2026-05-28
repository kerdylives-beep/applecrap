use std::{path::Path, process::Command};

use anyhow::{anyhow, Result};

pub fn open_external(target: &str) -> Result<()> {
    validate_external_target(target)?;

    let mut command = Command::new("rundll32.exe");
    command.args(["url.dll,FileProtocolHandler", target]);
    hide_command_window(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow!("unable to open {target}: {error}"))
}

fn validate_external_target(target: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(target.trim())
        .map_err(|_| anyhow!("Apple Music target must be a valid URL."))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("Apple Music target must include a host."))?
        .to_ascii_lowercase();

    if parsed.scheme() != "https" || host != "music.apple.com" {
        anyhow::bail!("Only https://music.apple.com links can be opened from AppleCrap Alpha.");
    }

    Ok(())
}

pub fn reveal_directory(path: &Path) -> Result<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow!("unable to reveal {}: {error}", path.display()))
}

pub fn launch_apple_music() -> Result<()> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
        "Start-Process",
        "shell:AppsFolder\\AppleInc.AppleMusicWin_nzyj5cx40ttqa!App",
    ]);
    hide_command_window(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow!("unable to launch Apple Music: {error}"))
}

#[cfg(windows)]
fn hide_command_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_command_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::validate_external_target;

    #[test]
    fn allows_apple_music_https_links() {
        assert!(validate_external_target(
            "https://music.apple.com/us/album/example/1?i=2&app=music"
        )
        .is_ok());
    }

    #[test]
    fn blocks_non_apple_music_links() {
        assert!(validate_external_target("https://example.com/us/album/1?i=2").is_err());
        assert!(validate_external_target("http://music.apple.com/us/album/1?i=2").is_err());
    }
}
