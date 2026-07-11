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
    models::{ProbeSession, ProbeSnapshot, QueueItem},
    services::queue_engine::{normalize_text, token_overlap},
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
    pub output_devices: Vec<crate::models::AudioOutputDevice>,
    pub current_sink: String,
    pub sink_error: Option<String>,
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

    /// Queue a track as Play Next. This is the only dispatch behaviour AppleCrap
    /// uses: requests are always queued rather than interrupting playback.
    pub async fn dispatch_track(&self, handle: &AppHandle, track_id: &str) -> Result<String> {
        self.run_command(handle, "queueNext", Some(track_id)).await
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
                output_devices: status.output_devices.clone(),
                current_output: status.current_sink.clone(),
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
                output_devices: status.output_devices.clone(),
                current_output: status.current_sink.clone(),
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

        let mut snapshot = snapshot_from_session(session, top_item);
        snapshot.source = "apple-music-web".to_string();
        snapshot.output_devices = status.output_devices.clone();
        snapshot.current_output = status.current_sink.clone();
        if let Some(sink_error) = status.sink_error.as_ref().filter(|e| !e.is_empty()) {
            snapshot.last_error = Some(format!("Audio routing: {sink_error}"));
        }

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

/// Build a snapshot from a single known media session (the embedded player
/// bridge gets its data from MusicKit rather than a PowerShell probe, but the
/// matching/confirmation logic below is shared with the old probe format).
pub fn snapshot_from_session(session: ProbeSession, top_item: Option<&QueueItem>) -> ProbeSnapshot {
    build_snapshot(
        ProbePayload {
            session: Some(session.clone()),
            sessions: vec![session],
            error: None,
        },
        top_item,
    )
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ProbePayload {
    session: Option<ProbeSession>,
    sessions: Vec<ProbeSession>,
    error: Option<String>,
}

fn build_snapshot(payload: ProbePayload, top_item: Option<&QueueItem>) -> ProbeSnapshot {
    let mut snapshot = ProbeSnapshot {
        sessions: payload.sessions,
        updated_at: Some(crate::models::now_iso()),
        ..ProbeSnapshot::default()
    };

    if let Some(error) = payload.error {
        snapshot.source = "error".to_string();
        snapshot.status = "Unavailable".to_string();
        snapshot.last_error = Some(error.clone());
        snapshot.explanation = error;
        return snapshot;
    }

    let Some(session) = payload.session else {
        snapshot.explanation =
            "No supported media session is visible to Windows right now.".to_string();
        return snapshot;
    };

    snapshot.source = if is_likely_apple_music(&session.app_id) {
        "apple-music".to_string()
    } else {
        "other-media".to_string()
    };
    snapshot.app_id = session.app_id.clone();
    snapshot.status = if session.status.is_empty() {
        "Unknown".to_string()
    } else {
        session.status.clone()
    };
    snapshot.title = session.title.clone();
    snapshot.artist = session.artist.clone();
    snapshot.album = session.album.clone();

    if let Some(item) = top_item {
        let (matched, confidence, explanation) = match_queue_item(&session, item);
        snapshot.matched = matched && snapshot.status.eq_ignore_ascii_case("playing");
        snapshot.matched_queue_id = snapshot.matched.then(|| item.id.clone());
        snapshot.confidence = confidence;
        snapshot.explanation = explanation;
    } else {
        snapshot.explanation =
            "No queue item is available to compare against playback.".to_string();
    }

    snapshot
}

fn is_likely_apple_music(app_id: &str) -> bool {
    let app_id = app_id.to_lowercase();
    app_id.contains("apple music")
        || app_id.contains("applemusic")
        || app_id.contains("appleinc.applemusic")
}

fn match_queue_item(session: &ProbeSession, queue_item: &QueueItem) -> (bool, f32, String) {
    let current_title = normalize_text(&session.title);
    let current_artist = normalize_text(&session.artist);
    let queue_title = normalize_text(
        queue_item
            .track
            .as_ref()
            .map(|track| track.title.as_str())
            .unwrap_or(queue_item.query.as_str()),
    );
    let queue_artist = normalize_text(
        queue_item
            .track
            .as_ref()
            .map(|track| track.artist_name.as_str())
            .unwrap_or_default(),
    );

    if current_title.is_empty() || queue_title.is_empty() {
        return (
            false,
            0.0,
            "Playback probe did not include a title, so the queue item could not be matched."
                .to_string(),
        );
    }

    let title_overlap = token_overlap(&queue_title, &current_title);
    let reverse_title_overlap = token_overlap(&current_title, &queue_title);
    let artist_overlap = if queue_artist.is_empty() {
        1.0
    } else {
        token_overlap(&queue_artist, &current_artist)
            .max(token_overlap(&current_artist, &queue_artist))
    };

    let title_matches =
        current_title == queue_title || (title_overlap >= 0.85 && reverse_title_overlap >= 0.85);
    let artist_matches = queue_artist.is_empty()
        || current_artist.contains(&queue_artist)
        || queue_artist.contains(&current_artist)
        || artist_overlap >= 0.34;

    let confidence =
        (((title_overlap.min(reverse_title_overlap)) * 0.75) + (artist_overlap * 0.25)).min(1.0);
    let matched = title_matches && artist_matches;
    let explanation = format!(
        "Title overlap {:.0}% / reverse {:.0}% and artist overlap {:.0}% between \"{}\" and the top queue item.",
        title_overlap * 100.0,
        reverse_title_overlap * 100.0,
        artist_overlap * 100.0,
        session.title
    );

    (matched, confidence, explanation)
}

#[cfg(test)]
mod probe_matching_tests {
    use super::*;
    use crate::models::{QueueHandoffState, QueueItem, ResolutionStatus, TrackMatch};

    #[test]
    fn matches_similar_titles_and_artists() {
        let session = ProbeSession {
            app_id: "Apple Music".to_string(),
            status: "Playing".to_string(),
            title: "Human Nature".to_string(),
            artist: "Michael Jackson".to_string(),
            album: String::new(),
        };

        let queue_item = QueueItem {
            id: "1".to_string(),
            requested_by: "viewer".to_string(),
            query: "human nature michael jackson".to_string(),
            submitted_at: crate::models::now_iso(),
            source: "twitch".to_string(),
            resolution: ResolutionStatus::Matched,
            track: Some(TrackMatch {
                id: "abc".to_string(),
                title: "Human Nature".to_string(),
                artist_name: "Michael Jackson".to_string(),
                album_name: "Thriller".to_string(),
                duration_ms: Some(240000),
                url: "https://example.com".to_string(),
                artwork_url: None,
            }),
            handoff_state: QueueHandoffState::SentToPlayer,
            resolved_track_url: Some("https://example.com".to_string()),
            match_confidence: Some(0.98),
            requires_manual_review: false,
            handoff_note: None,
            handoff_updated_at: None,
            dispatched_at: None,
        };

        let (matched, confidence, _) = match_queue_item(&session, &queue_item);
        assert!(matched);
        assert!(confidence > 0.5);
    }

    #[test]
    fn does_not_match_variant_titles() {
        let session = ProbeSession {
            app_id: "Apple Music".to_string(),
            status: "Playing".to_string(),
            title: "Moments In Love (Beaten)".to_string(),
            artist: "Art of Noise".to_string(),
            album: "Moments In Love".to_string(),
        };

        let queue_item = QueueItem {
            id: "1".to_string(),
            requested_by: "viewer".to_string(),
            query: "moments in love".to_string(),
            submitted_at: crate::models::now_iso(),
            source: "twitch".to_string(),
            resolution: ResolutionStatus::Matched,
            track: Some(TrackMatch {
                id: "abc".to_string(),
                title: "Moments In Love".to_string(),
                artist_name: "Art of Noise".to_string(),
                album_name: "Moments In Love".to_string(),
                duration_ms: Some(280000),
                url: "https://example.com".to_string(),
                artwork_url: None,
            }),
            handoff_state: QueueHandoffState::SentToPlayer,
            resolved_track_url: Some("https://example.com".to_string()),
            match_confidence: Some(0.81),
            requires_manual_review: false,
            handoff_note: None,
            handoff_updated_at: None,
            dispatched_at: None,
        };

        let (matched, _, _) = match_queue_item(&session, &queue_item);
        assert!(!matched);
    }
}
