use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use base64::Engine;
use serde::Deserialize;
use tauri::{AppHandle, Manager};
use tokio::sync::{oneshot, Mutex, RwLock};

use crate::{
    models::{AutomationHandoffMode, ProbeSession, ProbeSnapshot, QueueItem},
    services::now_playing_probe,
};

pub const PLAYER_WINDOW_LABEL: &str = "player";

/// How old a bridge status may be before we consider the player disconnected.
const STATUS_STALE_AFTER: Duration = Duration::from_secs(8);
/// How long a dispatched command may wait for the player to answer.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BridgeStatus {
    pub ready: bool,
    pub authorized: bool,
    pub playback_state: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub catalog_id: Option<String>,
    pub item_id: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum BridgeReport {
    Status(BridgeStatus),
    Result {
        tag: String,
        ok: bool,
        #[serde(default)]
        detail: String,
    },
}

struct CommandOutcome {
    ok: bool,
    detail: String,
}

pub struct PlayerBridge {
    latest: RwLock<Option<(BridgeStatus, Instant)>>,
    pending: Mutex<HashMap<String, oneshot::Sender<CommandOutcome>>>,
}

impl PlayerBridge {
    pub fn new() -> Self {
        Self {
            latest: RwLock::new(None),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Entry point for reports arriving from the player webview (IPC path).
    pub async fn handle_report(&self, payload: serde_json::Value) {
        match serde_json::from_value::<BridgeReport>(payload) {
            Ok(BridgeReport::Status(status)) => {
                let mut latest = self.latest.write().await;
                *latest = Some((status, Instant::now()));
            }
            Ok(BridgeReport::Result { tag, ok, detail }) => {
                let sender = self.pending.lock().await.remove(&tag);
                if let Some(sender) = sender {
                    let _ = sender.send(CommandOutcome { ok, detail });
                }
            }
            Err(_) => {}
        }
    }

    /// Fallback path: the bridge encodes reports into the window title when
    /// IPC from the remote origin is unavailable.
    async fn poll_title_channel(&self, handle: &AppHandle) {
        let Some(window) = handle.get_webview_window(PLAYER_WINDOW_LABEL) else {
            return;
        };
        let Ok(title) = window.title() else {
            return;
        };
        let Some(encoded) = title.strip_prefix("ACB1|") else {
            return;
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
            return;
        };
        let Ok(json) = String::from_utf8(bytes) else {
            return;
        };
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            self.handle_report(value).await;
        }
    }

    pub async fn current_status(&self, handle: &AppHandle) -> Option<BridgeStatus> {
        let fresh = {
            let latest = self.latest.read().await;
            latest
                .as_ref()
                .filter(|(_, at)| at.elapsed() < STATUS_STALE_AFTER)
                .map(|(status, _)| status.clone())
        };
        if fresh.is_some() {
            return fresh;
        }

        self.poll_title_channel(handle).await;
        let latest = self.latest.read().await;
        latest
            .as_ref()
            .filter(|(_, at)| at.elapsed() < STATUS_STALE_AFTER)
            .map(|(status, _)| status.clone())
    }

    /// Send one command into the player and wait for its result report.
    pub async fn run_command(
        &self,
        handle: &AppHandle,
        op: &str,
        track_id: Option<&str>,
    ) -> Result<String> {
        let window = handle
            .get_webview_window(PLAYER_WINDOW_LABEL)
            .ok_or_else(|| anyhow!("The player window is not available."))?;

        let tag = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(tag.clone(), sender);

        let script = format!(
            "window.__ACBRIDGE__ && window.__ACBRIDGE__.exec({op}, {id}, {tag});",
            op = serde_json::to_string(op)?,
            id = serde_json::to_string(&track_id.unwrap_or_default())?,
            tag = serde_json::to_string(&tag)?,
        );
        window.eval(&script)?;

        // The command result may arrive over the title channel; poll it while
        // we wait so the fallback path resolves too.
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let mut receiver = receiver;
        let outcome = loop {
            match tokio::time::timeout(Duration::from_millis(400), &mut receiver).await {
                Ok(Ok(outcome)) => break Some(outcome),
                Ok(Err(_)) => break None,
                Err(_) => {
                    self.poll_title_channel(handle).await;
                    if Instant::now() >= deadline {
                        break None;
                    }
                }
            }
        };
        self.pending.lock().await.remove(&tag);

        match outcome {
            Some(CommandOutcome { ok: true, detail }) => Ok(detail),
            Some(CommandOutcome { ok: false, detail }) => Err(anyhow!(detail)),
            None => Err(anyhow!(
                "The player did not answer in time. Make sure the player window is open and signed in."
            )),
        }
    }

    pub async fn dispatch_track(
        &self,
        handle: &AppHandle,
        track_id: &str,
        mode: &AutomationHandoffMode,
    ) -> Result<String> {
        let op = match mode {
            AutomationHandoffMode::PlayNow => "playNow",
            AutomationHandoffMode::PlayNext => "queueNext",
        };
        self.run_command(handle, op, Some(track_id)).await
    }

    /// Build a ProbeSnapshot from the latest bridge status so the existing
    /// confirmation loop and Now Playing UI keep working unchanged.
    pub async fn build_probe(
        &self,
        handle: &AppHandle,
        top_item: Option<&QueueItem>,
    ) -> (ProbeSnapshot, String) {
        let status = self.current_status(handle).await;

        let Some(status) = status else {
            let snapshot = ProbeSnapshot {
                source: "apple-music-web".to_string(),
                status: "Disconnected".to_string(),
                explanation:
                    "The embedded player has not reported yet. It may still be loading music.apple.com."
                        .to_string(),
                updated_at: Some(crate::models::now_iso()),
                ..ProbeSnapshot::default()
            };
            return (snapshot, String::new());
        };

        if !status.ready {
            let snapshot = ProbeSnapshot {
                source: "apple-music-web".to_string(),
                status: "Loading".to_string(),
                explanation: "The player is loading MusicKit. Give it a moment.".to_string(),
                updated_at: Some(crate::models::now_iso()),
                ..ProbeSnapshot::default()
            };
            return (snapshot, String::new());
        }

        if !status.authorized {
            let snapshot = ProbeSnapshot {
                source: "apple-music-web".to_string(),
                status: "SignInRequired".to_string(),
                explanation:
                    "The player is running but not signed in. Open the player window and sign in to Apple Music."
                        .to_string(),
                updated_at: Some(crate::models::now_iso()),
                ..ProbeSnapshot::default()
            };
            return (snapshot, String::new());
        }

        let session = ProbeSession {
            app_id: "Apple Music (embedded player)".to_string(),
            status: humanize_playback_state(&status.playback_state),
            title: status.title.clone(),
            artist: status.artist.clone(),
            album: status.album.clone(),
        };
        let signature = format!(
            "{}:{}:{}:{}",
            session.status, session.title, session.artist, status.catalog_id.as_deref().unwrap_or("")
        );

        let mut snapshot = now_playing_probe::snapshot_from_session(session, top_item);
        snapshot.source = "apple-music-web".to_string();

        // Exact catalog-id match beats fuzzy title matching when available.
        if let (Some(catalog_id), Some(item)) = (status.catalog_id.as_deref(), top_item) {
            if let Some(track) = item.track.as_ref() {
                if track.id == catalog_id {
                    snapshot.confidence = 1.0;
                    snapshot.matched = snapshot.status.eq_ignore_ascii_case("playing");
                    snapshot.matched_queue_id = snapshot.matched.then(|| item.id.clone());
                    snapshot.explanation = format!(
                        "Exact catalog match: the player reports catalog id {catalog_id} for \"{}\".",
                        status.title
                    );
                }
            }
        }

        (snapshot, signature)
    }
}

fn humanize_playback_state(state: &str) -> String {
    match state {
        "playing" => "Playing".to_string(),
        "paused" => "Paused".to_string(),
        "stopped" | "none" | "completed" | "ended" => "Stopped".to_string(),
        "loading" | "waiting" | "stalled" | "seeking" => "Loading".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Unknown".to_string(),
            }
        }
    }
}
