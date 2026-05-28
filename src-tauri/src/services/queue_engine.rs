use std::collections::HashSet;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::{AppSettings, QueueHandoffState, QueueItem, ResolutionStatus, TrackMatch};

pub fn normalize_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub fn extract_apple_music_track_id(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value).ok()?;
    let hostname = parsed.host_str()?.to_lowercase();
    if hostname != "music.apple.com" {
        return None;
    }

    let track_id = parsed.query_pairs().find_map(|(key, value)| {
        if key == "i" && value.chars().all(|character| character.is_ascii_digit()) {
            Some(value.to_string())
        } else {
            None
        }
    })?;

    Some(track_id)
}

pub fn validate_request(
    queue: &[QueueItem],
    settings: &AppSettings,
    requested_by: &str,
    query: &str,
    is_privileged: bool,
) -> Result<(), String> {
    let normalized_query = query.trim();
    if normalized_query.is_empty() {
        return Err("Please include a song title or artist.".to_string());
    }

    if !settings.request_limits.allow_links && is_url(normalized_query) {
        return Err("Links are disabled. Request by song title instead.".to_string());
    }

    if !is_privileged && queue.len() >= settings.request_limits.max_queue_size as usize {
        return Err("The request queue is full right now.".to_string());
    }

    let normalized_name = requested_by.to_lowercase();
    let user_requests = queue
        .iter()
        .filter(|item| item.requested_by.to_lowercase() == normalized_name)
        .collect::<Vec<_>>();

    if !is_privileged && user_requests.len() >= settings.request_limits.max_per_user as usize {
        return Err(format!(
            "You already have {} active request(s).",
            settings.request_limits.max_per_user
        ));
    }

    if !is_privileged && settings.request_limits.cooldown_seconds > 0 {
        let latest_request = user_requests
            .into_iter()
            .filter_map(|item| DateTime::parse_from_rfc3339(&item.submitted_at).ok())
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .max();

        if let Some(latest_request) = latest_request {
            let elapsed = Utc::now()
                .signed_duration_since(latest_request)
                .num_seconds();
            if elapsed < settings.request_limits.cooldown_seconds as i64 {
                return Err("Please wait before requesting another song.".to_string());
            }
        }
    }

    Ok(())
}

pub fn ensure_track_allowed(
    queue: &[QueueItem],
    settings: &AppSettings,
    track: &TrackMatch,
    is_privileged: bool,
) -> Result<(), String> {
    if !settings.request_limits.allow_duplicates
        && queue.iter().any(|item| {
            item.track.as_ref().map(|entry| entry.id.as_str()) == Some(track.id.as_str())
        })
    {
        return Err("That song is already in the queue.".to_string());
    }

    if !is_privileged {
        if let Some(duration_ms) = track.duration_ms {
            let max_duration_ms = settings.request_limits.max_track_minutes as i64 * 60 * 1000;
            if duration_ms > max_duration_ms {
                return Err(format!(
                    "Songs longer than {} minutes are blocked.",
                    settings.request_limits.max_track_minutes
                ));
            }
        }
    }

    Ok(())
}

pub fn build_queue_item(
    requested_by: &str,
    query: &str,
    source: &str,
    track: Option<TrackMatch>,
    match_confidence: Option<f32>,
) -> QueueItem {
    let resolution = if track.is_some() {
        ResolutionStatus::Matched
    } else {
        ResolutionStatus::ManualReview
    };
    let handoff_state = if track.is_some() {
        QueueHandoffState::PendingMatch
    } else {
        QueueHandoffState::ManualReview
    };

    QueueItem {
        id: Uuid::new_v4().to_string(),
        requested_by: requested_by.to_string(),
        query: query.trim().to_string(),
        submitted_at: crate::models::now_iso(),
        source: source.to_string(),
        resolution,
        resolved_track_url: track.as_ref().map(|entry| entry.url.clone()),
        match_confidence,
        requires_manual_review: track.is_none(),
        track,
        handoff_state,
        handoff_note: None,
        handoff_updated_at: None,
        dispatched_at: None,
    }
}

pub fn estimate_match_confidence(query: &str, title: &str, artist: &str) -> f32 {
    let query_normalized = normalize_text(query);
    let title_normalized = normalize_text(title);
    let artist_normalized = normalize_text(artist);
    let combined = normalize_text(&format!("{artist} {title}"));

    let exact_bonus = if query_normalized == title_normalized || query_normalized == combined {
        0.3
    } else {
        0.0
    };

    let title_overlap = token_overlap(&title_normalized, &query_normalized);
    let artist_overlap = token_overlap(&artist_normalized, &query_normalized);
    ((title_overlap * 0.65) + (artist_overlap * 0.35) + exact_bonus).min(1.0)
}

pub fn remove_latest_request_by_user(
    queue: &mut Vec<QueueItem>,
    requested_by: &str,
) -> Option<QueueItem> {
    let normalized_name = requested_by.to_lowercase();
    let index = queue
        .iter()
        .enumerate()
        .rev()
        .find(|(_, entry)| entry.requested_by.to_lowercase() == normalized_name)
        .map(|(index, _)| index)?;

    Some(queue.remove(index))
}

pub fn token_overlap(left: &str, right: &str) -> f32 {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);

    if left_tokens.is_empty() {
        return 0.0;
    }

    let matches = left_tokens
        .iter()
        .filter(|token| right_tokens.contains(*token))
        .count();

    matches as f32 / left_tokens.len() as f32
}

pub fn tokenize(value: &str) -> HashSet<String> {
    normalize_text(value)
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_apple_music_track_id() {
        let url =
            "https://music.apple.com/us/album/freefall-feat-durand-bernarr/1490035834?i=1490036368";
        assert_eq!(
            extract_apple_music_track_id(url).as_deref(),
            Some("1490036368")
        );
    }

    #[test]
    fn rejects_lookalike_apple_music_hosts() {
        let url = "https://evil-music.apple.com.example/us/album/freefall/1?i=1490036368";
        assert_eq!(extract_apple_music_track_id(url), None);
    }

    #[test]
    fn normalizes_text_consistently() {
        assert_eq!(
            normalize_text("Human Nature - Michael Jackson"),
            "human nature michael jackson"
        );
    }

    #[test]
    fn computes_token_overlap() {
        let overlap = token_overlap("human nature", "human nature michael jackson");
        assert!(overlap > 0.9);
    }
}
