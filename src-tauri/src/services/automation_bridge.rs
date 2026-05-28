use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;
use tokio::process::Command;

use crate::{
    models::{
        AppSettings, AutomationAction, AutomationAdapterKind, AutomationCapabilities,
        AutomationRunResult, QueueItem, RunAutomationPayload,
    },
    services::{apple_catalog::AppleCatalog, window_shell},
};

#[derive(Clone)]
pub struct AutomationBridge {
    script_path: PathBuf,
}

impl AutomationBridge {
    pub fn new(script_path: PathBuf) -> Self {
        Self { script_path }
    }

    pub fn capabilities() -> Vec<AutomationCapabilities> {
        vec![
            AutomationCapabilities {
                adapter: AutomationAdapterKind::DeepLink,
                supported_actions: vec![
                    AutomationAction::ProbeCapabilities,
                    AutomationAction::FocusPlayer,
                    AutomationAction::OpenTrack,
                    AutomationAction::DryRun,
                ],
                can_focus_player: true,
                can_open_track: true,
                can_queue_action: false,
                can_play: false,
            },
            AutomationCapabilities {
                adapter: AutomationAdapterKind::UiAutomation,
                supported_actions: vec![
                    AutomationAction::ProbeCapabilities,
                    AutomationAction::FocusPlayer,
                    AutomationAction::OpenTrack,
                    AutomationAction::AttemptPlay,
                    AutomationAction::AttemptQueueAction,
                    AutomationAction::DryRun,
                ],
                can_focus_player: true,
                can_open_track: true,
                can_queue_action: true,
                can_play: true,
            },
        ]
    }

    pub async fn run(
        &self,
        payload: &RunAutomationPayload,
        settings: &AppSettings,
        request: Option<&QueueItem>,
    ) -> AutomationRunResult {
        let timestamp = crate::models::now_iso();
        let action = requested_action(payload);
        let result = match payload.adapter {
            AutomationAdapterKind::DeepLink => self.run_deep_link(&action, settings, request).await,
            AutomationAdapterKind::UiAutomation => {
                self.run_ui_automation(&action, settings, request).await
            }
        };

        match result {
            Ok((summary, detail)) => AutomationRunResult {
                adapter: payload.adapter.clone(),
                action,
                ok: true,
                summary,
                detail,
                timestamp,
            },
            Err(error) => AutomationRunResult {
                adapter: payload.adapter.clone(),
                action,
                ok: false,
                summary: "Automation step failed.".to_string(),
                detail: error.to_string(),
                timestamp,
            },
        }
    }

    async fn run_deep_link(
        &self,
        action: &AutomationAction,
        settings: &AppSettings,
        request: Option<&QueueItem>,
    ) -> Result<(String, String)> {
        match action {
            AutomationAction::ProbeCapabilities => Ok((
                "Deep link adapter is healthy.".to_string(),
                "This adapter can open Apple Music tracks and launch the Apple Music app, but it does not manipulate the native queue.".to_string(),
            )),
            AutomationAction::FocusPlayer => {
                window_shell::launch_apple_music()?;
                Ok((
                    "Apple Music launch requested.".to_string(),
                    "Deep link automation attempted to foreground the Apple Music Windows app.".to_string(),
                ))
            }
            AutomationAction::OpenTrack => {
                let url = resolve_request_url(request, settings)?;
                window_shell::open_external(&url)?;
                Ok((
                    "Track opened in Apple Music.".to_string(),
                    format!("Deep link adapter opened {url}"),
                ))
            }
            AutomationAction::DryRun => Ok((
                "Deep link dry run complete.".to_string(),
                "No queue changes were made. The stable adapter would only open the track and wait for playback confirmation.".to_string(),
            )),
            AutomationAction::AttemptPlay | AutomationAction::AttemptQueueAction => Ok((
                "Deep link adapter cannot perform that action.".to_string(),
                "Stable automation intentionally avoids queue or playback mutation because it is not reliable on Windows Apple Music.".to_string(),
            )),
        }
    }

    async fn run_ui_automation(
        &self,
        action: &AutomationAction,
        settings: &AppSettings,
        request: Option<&QueueItem>,
    ) -> Result<(String, String)> {
        let url = resolve_request_url(request, settings).ok();
        let track_title = request
            .and_then(|item| item.track.as_ref().map(|track| track.title.clone()))
            .unwrap_or_default();
        let track_artist = request
            .and_then(|item| item.track.as_ref().map(|track| track.artist_name.clone()))
            .unwrap_or_default();
        let operation = match action {
            AutomationAction::ProbeCapabilities => "probe-capabilities",
            AutomationAction::FocusPlayer => "focus-player",
            AutomationAction::OpenTrack => "open-track",
            AutomationAction::AttemptQueueAction => "attempt-queue-action",
            AutomationAction::AttemptPlay => "attempt-play",
            AutomationAction::DryRun => "dry-run",
        };

        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&self.script_path)
            .arg("-Operation")
            .arg(operation)
            .arg("-TrackUrl")
            .arg(url.unwrap_or_default())
            .arg("-TrackTitle")
            .arg(track_title)
            .arg("-TrackArtist")
            .arg(track_artist);
        hide_command_window(&mut command);
        let output = command.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok((
                "UI automation finished with no output.".to_string(),
                "The PowerShell adapter did not return a payload.".to_string(),
            ));
        }

        let payload: ScriptResult = serde_json::from_str(&stdout)?;
        if payload.ok {
            Ok((payload.summary, payload.detail))
        } else {
            anyhow::bail!(payload.detail);
        }
    }
}

fn resolve_request_url(request: Option<&QueueItem>, settings: &AppSettings) -> Result<String> {
    let Some(request) = request else {
        anyhow::bail!("No queue item is selected for this automation step.")
    };

    if let Some(track) = request.track.as_ref() {
        return Ok(track.url.clone());
    }

    Ok(AppleCatalog::build_search_url(
        &request.query,
        &settings.apple_music.storefront,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptResult {
    ok: bool,
    summary: String,
    detail: String,
}

fn requested_action(payload: &RunAutomationPayload) -> AutomationAction {
    if payload.dry_run.unwrap_or(false) {
        AutomationAction::DryRun
    } else {
        payload.action.clone()
    }
}

#[cfg(windows)]
fn hide_command_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_command_window(_command: &mut Command) {}
