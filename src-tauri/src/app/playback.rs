//! The embedded player: now-playing probe, playback confirmation, media keys.

use super::*;

impl AppContext {
    /// Read-only snapshot of the latest Now Playing probe, used by the
    /// `!song` chat command. Kept separate from `snapshot()` so callers that
    /// only need the probe do not pay for cloning the queue/logs/settings.
    pub async fn current_probe(&self) -> ProbeSnapshot {
        self.runtime.read().await.probe.clone()
    }

    pub async fn run_probe_once(self: &Arc<Self>) -> Result<ProbeResult> {
        let snapshot = self.run_probe_cycle().await?;
        Ok(ProbeResult { snapshot })
    }

    pub async fn run_probe_cycle(self: &Arc<Self>) -> Result<ProbeSnapshot> {
        let top_item = self.persisted.read().await.queue.first().cloned();
        let (snapshot, session_signature) = self
            .player_bridge
            .build_probe(&self.handle, top_item.as_ref())
            .await;
        self.set_probe_snapshot(snapshot.clone(), session_signature)
            .await;
        self.note_player_status(&snapshot.status).await;

        #[cfg(desktop)]
        self.sync_media_key_claim(&snapshot).await;

        // Keep the player's audio output routed to the configured device. The
        // bridge reports its current sink; push the setting until they agree
        // (covers app start, player reloads, and settings changes alike).
        if snapshot.status != "Disconnected" {
            let desired = self.current_settings().await.player.audio_output_device;
            if snapshot.current_output != desired {
                let bridge_handle = self.handle.clone();
                let context = Arc::clone(self);
                tauri::async_runtime::spawn(async move {
                    let _ = context
                        .player_bridge
                        .run_command(&bridge_handle, "setSink", Some(&desired))
                        .await;
                });
            }
        }

        if snapshot.matched {
            let already_confirmed = self.runtime.read().await.last_confirmed_queue_id.clone();
            if already_confirmed.as_deref() != snapshot.matched_queue_id.as_deref() {
                if let Some(queue_id) = snapshot.matched_queue_id.clone() {
                    let can_confirm = top_item.as_ref().is_some_and(|item| {
                        matches!(
                            item.handoff_state,
                            QueueHandoffState::SentToPlayer | QueueHandoffState::ConfirmedPlaying
                        )
                    });
                    if !can_confirm {
                        return Ok(snapshot);
                    }
                    {
                        let mut runtime = self.runtime.write().await;
                        runtime.last_confirmed_queue_id = Some(queue_id.clone());
                    }
                    self.update_request_handoff(
                        &queue_id,
                        QueueHandoffState::ConfirmedPlaying,
                        Some("Playback confirmed by the Now Playing probe.".to_string()),
                    )
                    .await;
                    self.add_log(
                        LogLevel::Info,
                        format!(
                            "Now playing matched \"{}\". Removing it from the queue.",
                            top_item
                                .as_ref()
                                .and_then(|item| item
                                    .track
                                    .as_ref()
                                    .map(|track| track.title.clone()))
                                .unwrap_or_else(|| top_item
                                    .as_ref()
                                    .map(|item| item.query.clone())
                                    .unwrap_or_default())
                        ),
                    )
                    .await;
                    {
                        let mut runtime = self.runtime.write().await;
                        runtime.now_playing_request = top_item.clone();
                    }
                    let _ = self.remove_request(&queue_id).await;
                    self.ensure_queue_progress("playback confirmation").await;
                }
            }
        } else {
            let mut runtime = self.runtime.write().await;
            runtime.last_confirmed_queue_id = None;
        }

        Ok(snapshot)
    }

    /// Media keys are claimed only while the player actually holds a track.
    ///
    /// Windows routes media keys to whichever media session it considers
    /// "current", and that arbitration regularly favours a long-idle browser
    /// over the app that is actually playing. Claiming the keys outright is
    /// the only reliable fix, but claiming them permanently would steal them
    /// from other players — so the claim follows the music: held while a track
    /// is loaded (playing or paused), released as soon as it is not.
    #[cfg(desktop)]
    pub async fn sync_media_key_claim(self: &Arc<Self>, probe: &ProbeSnapshot) {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let enabled = self.current_settings().await.player.media_keys;
        let holds_track = matches!(probe.status.as_str(), "Playing" | "Paused");
        let want = enabled && holds_track;

        {
            let runtime = self.runtime.read().await;
            if runtime.media_keys_claimed == want {
                return;
            }
        }

        let manager = self.handle.global_shortcut();
        let shortcuts = media_key_shortcuts();
        let outcome = if want {
            shortcuts
                .iter()
                .try_for_each(|shortcut| manager.register(*shortcut))
        } else {
            shortcuts
                .iter()
                .try_for_each(|shortcut| manager.unregister(*shortcut))
        };

        match outcome {
            Ok(()) => {
                let mut runtime = self.runtime.write().await;
                runtime.media_keys_claimed = want;
            }
            Err(error) => {
                // Another app may hold the keys; try again on a later cycle.
                self.add_log(
                    LogLevel::Debug,
                    format!(
                        "Media key {} failed: {error}",
                        if want { "claim" } else { "release" }
                    ),
                )
                .await;
            }
        }
    }

    /// Handle one media key press by driving the embedded player directly.
    #[cfg(desktop)]
    pub async fn handle_media_key(self: &Arc<Self>, op: &str) {
        let result = self
            .player_bridge
            .run_command(&self.handle, op, None)
            .await;
        if let Err(error) = result {
            self.add_log(LogLevel::Warn, format!("Media key {op} failed: {error}"))
                .await;
        }
    }

    pub(super) async fn set_probe_snapshot(&self, snapshot: ProbeSnapshot, session_signature: String) {
        let mut should_log_sessions = None;
        let mut should_log_error = None;
        let mut should_log_devices = None;
        let unchanged;

        {
            let mut runtime = self.runtime.write().await;

            let devices_signature = snapshot
                .output_devices
                .iter()
                .map(|device| device.label.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            if !snapshot.output_devices.is_empty()
                && devices_signature != runtime.last_devices_signature
            {
                runtime.last_devices_signature = devices_signature.clone();
                should_log_devices = Some(format!(
                    "Player reports {} audio output device(s): {}",
                    snapshot.output_devices.len(),
                    devices_signature
                ));
            }
            if !session_signature.is_empty() && session_signature != runtime.last_session_signature
            {
                runtime.last_session_signature = session_signature;
                should_log_sessions = Some(
                    snapshot
                        .sessions
                        .iter()
                        .map(|session| {
                            format!(
                                "{}:{}:{}:{}",
                                session.status, session.title, session.artist, session.app_id
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
            }

            if let Some(error) = snapshot.last_error.clone() {
                if error != runtime.last_probe_error {
                    runtime.last_probe_error = error.clone();
                    should_log_error = Some(error);
                }
            } else {
                runtime.last_probe_error.clear();
            }

            // The probe runs every 2.5s forever. When nothing a viewer could
            // notice has changed, skip the broadcast entirely: emitting means
            // cloning the whole app state, serializing it, crossing IPC, and
            // re-rendering the dashboard.
            unchanged = runtime.probe.is_equivalent_to(&snapshot)
                && should_log_sessions.is_none()
                && should_log_error.is_none()
                && should_log_devices.is_none();
            runtime.probe = snapshot.clone();
        }

        if unchanged {
            return;
        }

        let _ = self.handle.emit("probeSnapshot", snapshot);
        self.emit_state().await;

        if let Some(summary) = should_log_sessions {
            self.add_log(
                LogLevel::Info,
                format!(
                    "Now Playing sessions: {}",
                    if summary.is_empty() {
                        "none detected"
                    } else {
                        &summary
                    }
                ),
            )
            .await;
        }

        if let Some(error) = should_log_error {
            self.add_log(LogLevel::Warn, format!("Now Playing unavailable: {error}"))
                .await;
        }

        if let Some(devices) = should_log_devices {
            self.add_log(LogLevel::Info, devices).await;
        }
    }
}

/// The media keys AppleCrap claims while it holds a track.
#[cfg(desktop)]
pub fn media_key_shortcuts() -> [tauri_plugin_global_shortcut::Shortcut; 3] {
    use tauri_plugin_global_shortcut::{Code, Shortcut};

    [
        Shortcut::new(None, Code::MediaPlayPause),
        Shortcut::new(None, Code::MediaTrackNext),
        Shortcut::new(None, Code::MediaTrackPrevious),
    ]
}

/// Map a pressed media key to a player bridge operation.
#[cfg(desktop)]
pub fn media_key_op(shortcut: &tauri_plugin_global_shortcut::Shortcut) -> Option<&'static str> {
    use tauri_plugin_global_shortcut::Code;

    match shortcut.key {
        Code::MediaPlayPause => Some("togglePlayPause"),
        Code::MediaTrackNext => Some("skip"),
        Code::MediaTrackPrevious => Some("previous"),
        _ => None,
    }
}
