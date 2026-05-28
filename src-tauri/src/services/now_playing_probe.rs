use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::Deserialize;
use tokio::process::Command;

use crate::{
    models::{ProbeSession, ProbeSnapshot, QueueItem},
    services::queue_engine::{normalize_text, token_overlap},
};

#[derive(Clone)]
pub struct NowPlayingProbe {
    script_path: PathBuf,
}

impl NowPlayingProbe {
    pub fn new(script_path: PathBuf) -> Self {
        Self { script_path }
    }

    pub async fn run(&self, top_item: Option<&QueueItem>) -> Result<ProbeRunResult> {
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
            .arg(&self.script_path);
        hide_command_window(&mut command);
        let output = command.output().await?;

        if !output.status.success() {
            return Err(anyhow!(
                "PowerShell probe exited with code {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(ProbeRunResult {
                snapshot: ProbeSnapshot::default(),
                session_signature: "[]".to_string(),
            });
        }

        let probe: ProbePayload = serde_json::from_str(&stdout)?;
        let snapshot = build_snapshot(probe, top_item);
        let session_signature =
            serde_json::to_string(&snapshot.sessions).unwrap_or_else(|_| "[]".to_string());
        Ok(ProbeRunResult {
            snapshot,
            session_signature,
        })
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

pub struct ProbeRunResult {
    pub snapshot: ProbeSnapshot,
    pub session_signature: String,
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
mod tests {
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
