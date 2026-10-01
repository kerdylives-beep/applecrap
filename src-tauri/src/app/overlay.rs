//! The OBS overlay server and the state it renders.

use crate::services::update_guard;

use super::*;

impl AppContext {
    /// Starts, stops, or rebinds the overlay server to match current settings.
    pub async fn sync_overlay_server(self: &Arc<Self>) {
        let settings = self.current_settings().await.overlay;
        let mut task = self.overlay_task.lock().await;

        let already_correct = matches!(task.as_ref(), Some((port, _)) if *port == settings.port)
            && settings.enabled;
        if already_correct {
            return;
        }

        if let Some((_, handle)) = task.take() {
            handle.abort();
        }

        if !settings.enabled {
            return;
        }

        let server_context = Arc::clone(self);
        let port = settings.port;
        let handle = tauri::async_runtime::spawn(async move {
            overlay_server::serve(server_context, port).await;
        });
        *task = Some((port, handle));
    }

    /// The overlay's port is taken. Right after an update that is almost
    /// always the previous version still holding it (versions before 0.6.1
    /// passed their sockets on to the version they started), so restart once
    /// to shake it loose. Otherwise, say so plainly.
    /// `overlay_port_busy` behind a boxed future: it can restart the overlay
    /// server, which can call it again, and the compiler can't prove such a
    /// future is `Send` without this break in the cycle.
    pub fn report_overlay_port_busy(
        self: &Arc<Self>,
        port: u16,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let context = Arc::clone(self);
        Box::pin(async move { context.overlay_port_busy(port).await })
    }

    async fn overlay_port_busy(self: &Arc<Self>, port: u16) {
        let version = self.handle.package_info().version.to_string();
        let relaunched = std::env::args().any(|arg| arg == update_guard::RELAUNCHED_FLAG);
        let layout = update_guard::Layout::current();
        let just_updated = layout
            .as_ref()
            .is_ok_and(|layout| update_guard::first_run_after_update(layout, &version));

        if just_updated && !relaunched {
            if let Ok(layout) = layout {
                self.add_log(
                    LogLevel::Info,
                    format!("The previous version is still holding the overlay's port {port}; restarting AppleCrap once to free it."),
                )
                .await;
                let context = Arc::clone(self);
                tauri::async_runtime::spawn(async move {
                    // Restarting before the update proves itself would look
                    // like a crash to the rollback watchdog.
                    let healthy = tauri::async_runtime::spawn_blocking(move || {
                        update_guard::wait_until_healthy(&layout, std::time::Duration::from_secs(180))
                    })
                    .await
                    .unwrap_or(false);
                    if !healthy {
                        return;
                    }
                    // Give the watchdog a moment to see it and step down.
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    context.restart_when_port_free(port).await;
                });
                return;
            }
        }

        self.raise_alert(crate::models::Alert {
            id: "overlay-port".into(),
            tone: crate::models::AlertTone::Warn,
            title: "The OBS overlay couldn't start".into(),
            detail: format!(
                "Port {port} is in use. Close and reopen AppleCrap; if that doesn't fix it, pick another port on the Overlay page."
            ),
            action: None,
        })
        .await;
    }

    /// Hands off to a helper that starts the app again once nothing holds
    /// the overlay's port, then exits.
    async fn restart_when_port_free(self: &Arc<Self>, port: u16) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        self.release_network_for_restart().await;
        self.flush_pending_state().await;
        match update_guard::spawn_clean(&exe, &[update_guard::RELAUNCH_FLAG, &port.to_string()]) {
            Ok(_) => self.handle.exit(0),
            Err(error) => {
                self.add_log(LogLevel::Warn, format!("Couldn't restart AppleCrap: {error}"))
                    .await;
                self.sync_overlay_server().await;
            }
        }
    }

    /// Snapshot for the OBS overlay: what is playing, who asked for it, and
    /// what is queued behind it.
    pub async fn overlay_state(&self) -> OverlayState {
        let persisted = self.persisted.read().await;
        let runtime = self.runtime.read().await;
        let probe = &runtime.probe;
        let settings = &persisted.settings.overlay;

        // Paused and loading still count as "there is a current track". Only
        // strictly-playing would blink the overlay off during every track
        // change, when the player briefly reports something else.
        let playing = !probe.title.is_empty()
            && ["playing", "paused", "loading"]
                .iter()
                .any(|state| probe.status.eq_ignore_ascii_case(state));

        let request = current_request(&runtime);
        let paused = playing && !probe.status.eq_ignore_ascii_case("playing");

        let queue = persisted
            .queue
            .iter()
            .take(settings.queue_count.clamp(1, 10) as usize)
            .map(|item| OverlayQueueItem {
                title: item
                    .track
                    .as_ref()
                    .map(|track| track.title.clone())
                    .unwrap_or_else(|| item.query.clone()),
                artist: item
                    .track
                    .as_ref()
                    .map(|track| track.artist_name.clone())
                    .unwrap_or_default(),
                requested_by: item.requested_by.clone(),
            })
            .collect();

        let hint = settings.show_hint.then(|| {
            request_hint(
                &persisted.settings,
                runtime.channel_points_status.phase == crate::models::ChannelPointsPhase::Live,
            )
        });

        OverlayState {
            playing,
            paused,
            title: probe.title.clone(),
            artist: probe.artist.clone(),
            album: probe.album.clone(),
            artwork_url: probe.artwork_url.clone(),
            requested_with_points: request
                .as_ref()
                .is_some_and(|request| request.source == "channel-points"),
            requested_by: request.map(|request| request.requested_by),
            position_ms: current_position(probe, chrono::Utc::now()),
            duration_ms: probe.duration_ms,
            show_queue: settings.show_queue,
            queue,
            hint,
            style: settings.style.clone(),
            art_colors: settings.art_colors,
            show_while_paused: settings.show_while_paused,
            pop_up_seconds: settings.pop_up_seconds,
        }
    }
}

/// The request behind the track playing now. Only credited while it still
/// matches what the player reports, so attribution can't go stale across
/// tracks.
pub(super) fn current_request(runtime: &RuntimeState) -> Option<crate::models::NowPlayingRequest> {
    let probe = &runtime.probe;
    let playing = !probe.title.is_empty()
        && ["playing", "paused", "loading"]
            .iter()
            .any(|state| probe.status.eq_ignore_ascii_case(state));
    runtime.now_playing_request.as_ref().and_then(|item| {
        let matches_track = item
            .track
            .as_ref()
            .map(|track| {
                queue_engine::normalize_text(&track.title)
                    == queue_engine::normalize_text(&probe.title)
            })
            .unwrap_or(false);
        (matches_track && playing).then(|| crate::models::NowPlayingRequest {
            requested_by: item.requested_by.clone(),
            source: item.source.clone(),
        })
    })
}

/// The track position now: the player's last report, advanced by the time
/// since while it's playing. Reports only arrive every couple of seconds.
fn current_position(probe: &ProbeSnapshot, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    let reported = probe.position_ms?;
    let mut position = reported;
    if probe.status.eq_ignore_ascii_case("playing") {
        let since = probe
            .updated_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| now.signed_duration_since(at).num_milliseconds().max(0))
            .unwrap_or(0);
        position += since;
    }
    Some(match probe.duration_ms {
        Some(duration) => position.min(duration),
        None => position,
    })
}

/// How viewers can ask for a song, in one line.
fn request_hint(settings: &crate::models::AppSettings, points_live: bool) -> String {
    let command = settings.twitch.request_command.trim();
    let cost = settings.channel_points.cost;
    match (points_live, settings.channel_points.points_only) {
        (true, true) => format!("Request a song: {cost} channel points"),
        (true, false) => format!("Request a song: {command} + name, or {cost} points"),
        _ => format!("Request a song: {command} + name"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(status: &str, position: i64, updated: &str) -> ProbeSnapshot {
        ProbeSnapshot {
            status: status.to_string(),
            position_ms: Some(position),
            duration_ms: Some(200_000),
            updated_at: Some(updated.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn position_advances_only_while_playing() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:03Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let reported = "2026-09-29T12:00:00Z";
        assert_eq!(current_position(&probe("Playing", 10_000, reported), now), Some(13_000));
        assert_eq!(current_position(&probe("Paused", 10_000, reported), now), Some(10_000));
        // Never past the end of the track.
        assert_eq!(current_position(&probe("Playing", 199_000, reported), now), Some(200_000));
    }

    #[test]
    fn hint_matches_how_requests_are_taken() {
        let mut settings = crate::models::AppSettings::default();
        settings.twitch.request_command = "!sr".into();
        settings.channel_points.cost = 500;
        assert_eq!(request_hint(&settings, false), "Request a song: !sr + name");
        assert_eq!(request_hint(&settings, true), "Request a song: !sr + name, or 500 points");
        settings.channel_points.points_only = true;
        assert_eq!(request_hint(&settings, true), "Request a song: 500 channel points");
        // Points-only only applies while the reward is actually live.
        assert_eq!(request_hint(&settings, false), "Request a song: !sr + name");
    }
}
