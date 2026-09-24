//! The OBS overlay server and the state it renders.

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

        // Only credit the stored requester while it still matches what the
        // player reports, so attribution cannot go stale across tracks.
        let requested_by = runtime.now_playing_request.as_ref().and_then(|item| {
            let matches_track = item
                .track
                .as_ref()
                .map(|track| {
                    queue_engine::normalize_text(&track.title)
                        == queue_engine::normalize_text(&probe.title)
                })
                .unwrap_or(false);
            (matches_track && playing).then(|| item.requested_by.clone())
        });

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

        OverlayState {
            playing,
            title: probe.title.clone(),
            artist: probe.artist.clone(),
            album: probe.album.clone(),
            artwork_url: probe.artwork_url.clone(),
            requested_by,
            show_queue: settings.show_queue,
            queue,
        }
    }
}
