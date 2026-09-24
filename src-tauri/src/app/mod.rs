//! `AppContext`: the app's shared state and the operations on it.
//!
//! Split by concern; each submodule adds its methods to `AppContext`:
//! - `queue` — request intake, matching, approval and dispatch
//! - `playback` — the now-playing probe, playback confirmation, media keys
//! - `twitch` — the chat connection and the identity it logs in with
//! - `overlay` — the OBS overlay server
//! - `maintenance` — updates, diagnostics, legacy import
//!
//! This file holds construction, background services, snapshots, logging
//! and persistence.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Result};
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::{
    models::{
        compact_log_message, AppState, AppStats, ApproveRequestPayload, AuthSlot, BotConnectionState,
        BotStatus, CommandResult, DiagnosticsSnapshot, LegacyImportStatus, LogEntry, LogLevel,
        OpenTrackPayload, OverlayQueueItem, OverlayState, PersistedState, ProbeResult,
        ProbeSnapshot, QueueHandoffState, QueueItem, ResolutionStatus, SaveSettingsPayload,
        SearchResult,
    },
    services::{
        apple_catalog::AppleCatalog, diagnostics, overlay_server, player_bridge::PlayerBridge,
        queue_engine, settings_store::SettingsStore, twitch_service, updater, window_shell,
    },
};

mod auth;
mod channel_points;
mod maintenance;
mod overlay;
mod playback;
mod queue;
mod twitch;

pub use playback::media_key_op;

pub struct AppContext {
    pub handle: AppHandle,
    pub storage: SettingsStore,
    pub player_bridge: PlayerBridge,
    /// One shared HTTP client for every outbound call. It carries the
    /// connect and request timeouts; reqwest's default has none, so a single
    /// stuck request could hang whatever was waiting on it indefinitely.
    pub http: reqwest::Client,
    /// Set when persisted state has unwritten changes awaiting the debounced
    /// flush (see `mark_persist_dirty`).
    pending_persist: AtomicBool,
    /// Serializes state saves (see `save_persisted`).
    save_lock: Mutex<()>,
    apple_catalog: AppleCatalog,
    persisted: RwLock<PersistedState>,
    runtime: RwLock<RuntimeState>,
    twitch_connection: Mutex<Option<TwitchConnection>>,
    /// Running overlay server, with the port it was bound to so a settings
    /// change can tell whether it needs restarting.
    overlay_task: Mutex<Option<(u16, JoinHandle<()>)>>,
    /// Serializes Twitch token refreshes (refresh tokens are one-time use).
    auth_refresh_lock: Mutex<()>,
    /// The background poll for an in-progress device-code sign-in.
    sign_in_task: Mutex<Option<JoinHandle<()>>>,
    /// Serializes Channel Points reward setup.
    channel_points_sync: Mutex<()>,
    /// The redemption listener, with the reward id it listens to.
    channel_points_listener: Mutex<Option<(String, JoinHandle<()>)>>,
    /// A pending retry of Channel Points setup after a network failure.
    channel_points_retry: Mutex<Option<JoinHandle<()>>>,
    /// Recently handled redemption ids (see `first_sighting`).
    seen_redemptions: std::sync::Mutex<std::collections::VecDeque<String>>,
}

struct TwitchConnection {
    id: u64,
    writer: mpsc::UnboundedSender<twitch_service::Outbound>,
    task: JoinHandle<()>,
}

struct RuntimeState {
    bot_status: BotStatus,
    probe: ProbeSnapshot,
    diagnostics: DiagnosticsSnapshot,
    legacy_import: LegacyImportStatus,
    last_confirmed_queue_id: Option<String>,
    last_session_signature: String,
    last_devices_signature: String,
    last_probe_error: String,
    auto_handoff_in_flight: bool,
    update: Option<crate::models::UpdateInfo>,
    media_keys_claimed: bool,
    /// The request whose track is currently playing. Kept because confirming
    /// playback removes the item from the queue, and the overlay still wants
    /// to credit whoever asked for it.
    now_playing_request: Option<QueueItem>,
    pending_sign_in: Option<crate::models::PendingSignIn>,
    auth_error: Option<String>,
    channel_points_status: crate::models::ChannelPointsStatus,
    alerts: Vec<crate::models::Alert>,
}

impl AppContext {
    pub fn initialize(handle: AppHandle) -> Result<Self> {
        let mut storage = SettingsStore::resolve()?;
        crate::services::crash_report::install(&storage.data_dir, env!("CARGO_PKG_VERSION"));
        let previous_session = crate::services::crash_report::begin_session(&storage.data_dir);
        let _ = storage.append_runtime_log(&format!(
            "[INFO] {} AppleCrap {} starting. data_dir={}",
            crate::models::now_iso(),
            env!("CARGO_PKG_VERSION"),
            storage.data_dir.display()
        ));
        let mut alerts = Vec::new();
        if previous_session.unclean {
            let _ = storage.append_runtime_log(&format!(
                "[WARN] {} The last session didn't close normally.",
                crate::models::now_iso()
            ));
            if let Some(crash) = previous_session.crash {
                alerts.push(crash_alert(
                    "last-crash",
                    "AppleCrap crashed last time",
                    &crash.message,
                ));
            }
        }
        let persisted = storage.load_persisted_state();
        let _ = storage.save_persisted_state(&persisted);
        // Only offer the legacy Electron import while the app is unconfigured;
        // once a bot is set up the old data is noise.
        let legacy_import = if persisted.settings.twitch.oauth_token.trim().is_empty()
            && persisted.settings.twitch.channel.trim().is_empty()
        {
            storage.detect_legacy_import()
        } else {
            LegacyImportStatus::default()
        };

        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("AppleCrap/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            handle,
            player_bridge: PlayerBridge::new(),
            pending_persist: AtomicBool::new(false),
            save_lock: Mutex::new(()),
            apple_catalog: AppleCatalog::new(http.clone()),
            http,
            storage,
            persisted: RwLock::new(persisted),
            runtime: RwLock::new(RuntimeState {
                bot_status: BotStatus::default(),
                probe: ProbeSnapshot::default(),
                diagnostics: DiagnosticsSnapshot {
                    last_summary: "Alpha diagnostics ready.".to_string(),
                    ..Default::default()
                },
                legacy_import,
                last_confirmed_queue_id: None,
                last_session_signature: String::new(),
                last_devices_signature: String::new(),
                last_probe_error: String::new(),
                auto_handoff_in_flight: false,
                update: None,
                media_keys_claimed: false,
                now_playing_request: None,
                pending_sign_in: None,
                auth_error: None,
                channel_points_status: Default::default(),
                alerts,
            }),
            twitch_connection: Mutex::new(None),
            overlay_task: Mutex::new(None),
            auth_refresh_lock: Mutex::new(()),
            sign_in_task: Mutex::new(None),
            channel_points_sync: Mutex::new(()),
            channel_points_listener: Mutex::new(None),
            channel_points_retry: Mutex::new(None),
            seen_redemptions: std::sync::Mutex::new(Default::default()),
        })
    }

    pub fn start_background_services(self: &Arc<Self>) {
        let probe_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                let _ = probe_context.run_probe_cycle().await;
                tokio::time::sleep(Duration::from_millis(2500)).await;
            }
        });

        let auto_connect_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            if auto_connect_context
                .current_settings()
                .await
                .twitch
                .auto_connect
            {
                if let Err(error) = auto_connect_context.connect_bot().await {
                    auto_connect_context
                        .add_log(LogLevel::Error, format!("Auto-connect failed: {error}"))
                        .await;
                }
            }
        });

        // A panic in a background task stops only that task, so the app
        // looks fine while something (chat, playback) quietly isn't. Say so.
        let panic_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Some(crash) = crate::services::crash_report::take_runtime_panic() {
                    panic_context
                        .raise_alert(crash_alert(
                            "runtime-panic",
                            "Part of AppleCrap stopped working",
                            &crash.message,
                        ))
                        .await;
                }
            }
        });

        let update_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let _ = update_context.check_for_updates().await;
        });

        let overlay_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            overlay_context.sync_overlay_server().await;
        });

        let channel_points_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            channel_points_context.sync_channel_points().await;
        });

        // Twitch requires validating signed-in tokens at startup and hourly.
        let validation_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(15)).await;
            loop {
                validation_context.validate_signed_in_accounts().await;
                tokio::time::sleep(Duration::from_secs(60 * 60)).await;
            }
        });

        // Debounced writer for low-stakes state changes (log lines). Queue and
        // settings mutations still write through immediately.
        let persist_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                if persist_context.pending_persist.load(Ordering::Relaxed) {
                    let _ = persist_context.save_persisted().await;
                }
            }
        });

        // Resume auto-queue for requests that were pending when the app last
        // closed. Delayed so the embedded player has time to load MusicKit.
        let resume_context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            resume_context.ensure_queue_progress("startup").await;
        });
    }

    pub async fn snapshot(&self) -> AppState {
        let persisted = self.persisted.read().await;
        let runtime = self.runtime.read().await;
        let matched_requests = persisted
            .queue
            .iter()
            .filter(|item| item.track.is_some())
            .count();
        let ready_request = persisted
            .queue
            .iter()
            .find(|item| {
                matches!(
                    item.handoff_state,
                    QueueHandoffState::ReadyToSend
                        | QueueHandoffState::ManualReview
                        | QueueHandoffState::PendingMatch
                )
            })
            .cloned();

        AppState {
            settings: persisted.settings.clone(),
            queue: persisted.queue.clone(),
            ready_request,
            logs: persisted.logs.iter().take(80).cloned().collect(),
            bot_status: runtime.bot_status.clone(),
            probe: runtime.probe.clone(),
            diagnostics: runtime.diagnostics.clone(),
            legacy_import: runtime.legacy_import.clone(),
            storage: self.storage.storage.clone(),
            update: runtime.update.clone(),
            auth: self.auth_summary(&persisted, &runtime),
            channel_points: runtime.channel_points_status.clone(),
            alerts: runtime.alerts.clone(),
            stats: AppStats {
                total_requests: persisted.queue.len(),
                unresolved_requests: persisted
                    .queue
                    .iter()
                    .filter(|item| item.track.is_none())
                    .count(),
                matched_requests,
                connected_since: runtime.bot_status.last_event_at.clone(),
            },
        }
    }

    pub async fn emit_state(&self) {
        let snapshot = self.snapshot().await;
        let _ = self.handle.emit("stateChanged", snapshot);
    }

    pub async fn add_log(&self, level: LogLevel, message: impl Into<String>) {
        let message = message.into();
        let entry = LogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            message: compact_log_message(&message),
            timestamp: crate::models::now_iso(),
        };
        let runtime_line = format!(
            "[{}] {} {}",
            log_level_label(&entry.level),
            entry.timestamp,
            message
        );

        {
            let mut persisted = self.persisted.write().await;
            persisted.logs.insert(0, entry.clone());
            persisted.logs.truncate(150);
        }

        let _ = self.storage.append_runtime_log(&runtime_line);
        // Log lines are already durable in runtime.log; the copy inside
        // state.json can ride the debounced flush instead of forcing a full
        // serialize + file write per line.
        self.mark_persist_dirty();
        // Only the new line goes out; the UI appends it. Re-sending the whole
        // app state here meant every log line (several per chat request)
        // serialized everything, crossed IPC and re-rendered the dashboard.
        let _ = self.handle.emit("logAppended", entry);
    }

    pub async fn save_persisted(&self) -> Result<()> {
        // One save at a time, so a slower write of an older snapshot can never
        // land after a newer one.
        let _write_turn = self.save_lock.lock().await;
        // Clear the flag before taking the snapshot: a change that races in
        // afterwards re-marks it and is picked up by the next flush, instead
        // of being cleared without ever being written.
        self.pending_persist.store(false, Ordering::Relaxed);
        let state = self.persisted.read().await.clone();
        let target = self.storage.state_target();
        // File I/O off the async runtime; saves include an fsync.
        tokio::task::spawn_blocking(move || target.save(&state))
            .await
            .map_err(|error| anyhow!("save task failed: {error}"))?
    }

    /// Queue a state write for the background flusher. Used for low-stakes,
    /// high-frequency changes (log lines); queue and settings mutations still
    /// persist immediately via `save_persisted`.
    fn mark_persist_dirty(&self) {
        self.pending_persist.store(true, Ordering::Relaxed);
    }

    /// Write out any debounced changes. Called on shutdown so a queued log
    /// flush is never lost when the app closes.
    pub async fn flush_pending_state(&self) {
        if self.pending_persist.load(Ordering::Relaxed) {
            let _ = self.save_persisted().await;
        }
    }

    pub async fn current_settings(&self) -> crate::models::AppSettings {
        self.persisted.read().await.settings.clone()
    }

    pub async fn save_settings(self: &Arc<Self>, payload: SaveSettingsPayload) -> Result<AppState> {
        let channel_points_changed = {
            // Merge under the write lock. Reading, merging, then writing in a
            // separate step let two quick changes (say, two toggles flipped in
            // a row) both start from the same copy, and one of them was lost.
            let mut persisted = self.persisted.write().await;
            let before = (
                persisted.settings.channel_points.clone(),
                persisted.settings.request_limits.allow_links,
            );
            persisted.settings.merge_patch(payload);
            before
                != (
                    persisted.settings.channel_points.clone(),
                    persisted.settings.request_limits.allow_links,
                )
        };

        self.save_persisted().await?;
        self.emit_state().await;
        self.sync_overlay_server().await;
        if channel_points_changed {
            // Talks to Twitch, so don't hold up the save on it.
            let context = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                context.sync_channel_points().await;
            });
        }
        self.ensure_queue_progress("settings update").await;
        Ok(self.snapshot().await)
    }
}

impl AppContext {
    /// Marks this run as having ended cleanly.
    pub fn end_session(&self) {
        crate::services::crash_report::end_session(&self.storage.data_dir);
    }
}

fn crash_alert(id: &str, title: &str, message: &str) -> crate::models::Alert {
    let message: String = message.chars().take(200).collect();
    crate::models::Alert {
        id: id.to_string(),
        tone: crate::models::AlertTone::Error,
        title: title.to_string(),
        detail: format!(
            "{message}. Restarting usually gets things going again; sending a report helps get it fixed."
        ),
        action: Some(crate::models::AlertAction::ReportProblem),
    }
}

fn log_level_label(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
        LogLevel::Debug => "DEBUG",
    }
}
