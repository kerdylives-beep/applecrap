use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::{
    models::{
        compact_log_message, AppState, AppStats, ApproveRequestPayload, AutomationControlMode,
        AutomationRunResult, AutomationSnapshot, BotConnectionState, BotStatus, CommandResult,
        DiagnosticsSnapshot, LegacyImportStatus, LogEntry, LogLevel, OpenTrackPayload,
        PersistedState, ProbeResult, ProbeSnapshot, QueueHandoffState, QueueItem, ResolutionStatus,
        RunAutomationPayload, SaveSettingsPayload, SearchResult,
    },
    services::{
        apple_catalog::AppleCatalog, automation_bridge::AutomationBridge, diagnostics,
        now_playing_probe::NowPlayingProbe, queue_engine, settings_store::SettingsStore,
        twitch_service, window_shell,
    },
};

pub struct AppContext {
    pub handle: AppHandle,
    pub storage: SettingsStore,
    apple_catalog: AppleCatalog,
    automation_bridge: AutomationBridge,
    probe_service: NowPlayingProbe,
    persisted: RwLock<PersistedState>,
    runtime: RwLock<RuntimeState>,
    twitch_connection: Mutex<Option<TwitchConnection>>,
}

struct TwitchConnection {
    writer: mpsc::UnboundedSender<String>,
    task: JoinHandle<()>,
}

struct RuntimeState {
    bot_status: BotStatus,
    probe: ProbeSnapshot,
    automation: AutomationSnapshot,
    diagnostics: DiagnosticsSnapshot,
    legacy_import: LegacyImportStatus,
    last_confirmed_queue_id: Option<String>,
    last_session_signature: String,
    last_probe_error: String,
    auto_handoff_in_flight: bool,
}

impl AppContext {
    pub fn initialize(handle: AppHandle) -> Result<Self> {
        let storage = SettingsStore::resolve()?;
        storage.write_support_scripts()?;
        let _ = storage.append_runtime_log(&format!(
            "[INFO] {} AppleCrap Alpha starting. data_dir={}",
            crate::models::now_iso(),
            storage.data_dir.display()
        ));
        let persisted = storage.load_persisted_state();
        let _ = storage.save_persisted_state(&persisted);
        let legacy_import = storage.detect_legacy_import();
        let automation = AutomationSnapshot {
            active_adapter: persisted.settings.automation.adapter.clone(),
            experimental_enabled: persisted
                .settings
                .automation
                .experimental_automation_enabled,
            capabilities: AutomationBridge::capabilities(),
            last_run: None,
        };

        Ok(Self {
            handle,
            apple_catalog: AppleCatalog::new(),
            automation_bridge: AutomationBridge::new(
                storage.scripts_dir.join("apple-music-automation.ps1"),
            ),
            probe_service: NowPlayingProbe::new(storage.scripts_dir.join("now-playing-probe.ps1")),
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
                automation,
                last_confirmed_queue_id: None,
                last_session_signature: String::new(),
                last_probe_error: String::new(),
                auto_handoff_in_flight: false,
            }),
            twitch_connection: Mutex::new(None),
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
            control_mode: persisted.settings.automation.control_mode.clone(),
            logs: persisted.logs.iter().take(80).cloned().collect(),
            bot_status: runtime.bot_status.clone(),
            probe: runtime.probe.clone(),
            automation: runtime.automation.clone(),
            diagnostics: runtime.diagnostics.clone(),
            legacy_import: runtime.legacy_import.clone(),
            storage: self.storage.storage.clone(),
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
        let _ = self.save_persisted().await;
        let _ = self.handle.emit("logAppended", entry);
        self.emit_state().await;
    }

    pub async fn save_persisted(&self) -> Result<()> {
        let state = self.persisted.read().await.clone();
        self.storage.save_persisted_state(&state)
    }

    pub async fn current_settings(&self) -> crate::models::AppSettings {
        self.persisted.read().await.settings.clone()
    }

    pub fn current_settings_blocking(&self) -> crate::models::AppSettings {
        self.persisted.blocking_read().settings.clone()
    }

    pub async fn save_settings(self: &Arc<Self>, payload: SaveSettingsPayload) -> Result<AppState> {
        let next_settings = {
            let persisted = self.persisted.read().await;
            let mut next = persisted.settings.clone();
            next.merge_patch(payload);
            next
        };

        self.apply_dispatch_hotkey(&next_settings.automation.dispatch_hotkey)
            .await?;

        {
            let mut persisted = self.persisted.write().await;
            persisted.settings = next_settings;
        }

        {
            let settings = self.persisted.read().await.settings.clone();
            let mut runtime = self.runtime.write().await;
            runtime.automation.active_adapter = settings.automation.adapter.clone();
            runtime.automation.experimental_enabled =
                settings.automation.experimental_automation_enabled;
        }

        self.save_persisted().await?;
        self.emit_state().await;
        self.ensure_queue_progress("settings update").await;
        Ok(self.snapshot().await)
    }

    pub async fn connect_bot(self: &Arc<Self>) -> Result<AppState> {
        twitch_service::connect(Arc::clone(self)).await?;
        Ok(self.snapshot().await)
    }

    pub async fn disconnect_bot(self: &Arc<Self>) -> Result<AppState> {
        twitch_service::disconnect(Arc::clone(self)).await?;
        Ok(self.snapshot().await)
    }

    pub async fn process_request(
        self: &Arc<Self>,
        requested_by: &str,
        query: &str,
        is_privileged: bool,
        source: &str,
    ) -> CommandResult {
        let query = query.trim();
        let snapshot = self.persisted.read().await.clone();

        if let Err(message) = queue_engine::validate_request(
            &snapshot.queue,
            &snapshot.settings,
            requested_by,
            query,
            is_privileged,
        ) {
            return CommandResult::error(message);
        }

        let mut track = None;
        let mut match_confidence = None;
        match self
            .apple_catalog
            .search_top_track(query, &snapshot.settings.apple_music)
            .await
        {
            Ok(found) => {
                if let Some(found_track) = found {
                    if let Err(message) = queue_engine::ensure_track_allowed(
                        &snapshot.queue,
                        &snapshot.settings,
                        &found_track,
                        is_privileged,
                    ) {
                        return CommandResult::error(message);
                    }
                    match_confidence = Some(queue_engine::estimate_match_confidence(
                        query,
                        &found_track.title,
                        &found_track.artist_name,
                    ));
                    track = Some(found_track);
                } else {
                    self.add_log(
                        LogLevel::Info,
                        format!(
                            "No Apple Music song match was found for \"{query}\". Falling back to manual review."
                        ),
                    )
                    .await;
                }
            }
            Err(error) => {
                self.add_log(
                    LogLevel::Warn,
                    format!("Apple Music lookup failed for \"{query}\": {error}"),
                )
                .await;
            }
        }

        let request = queue_engine::build_queue_item(
            requested_by,
            query,
            source,
            track.clone(),
            match_confidence,
        );
        {
            let mut persisted = self.persisted.write().await;
            persisted.queue.push(request.clone());
        }

        let _ = self.save_persisted().await;
        self.emit_state().await;

        if let Some(track) = track {
            self.add_log(
                LogLevel::Info,
                format!("Queued \"{}\" for {}.", track.title, requested_by),
            )
            .await;
            self.ensure_queue_progress("new matched request").await;
            CommandResult::ok(format!("Queued {} by {}.", track.title, track.artist_name))
        } else {
            self.add_log(
                LogLevel::Info,
                format!("Queued manual review request \"{query}\" for {requested_by}."),
            )
            .await;
            CommandResult::ok(format!("Saved \"{query}\" for manual Apple Music review."))
        }
    }

    pub async fn enqueue_manual_request(
        self: &Arc<Self>,
        requested_by: &str,
        query: &str,
    ) -> CommandResult {
        self.process_request(requested_by, query, true, "dashboard")
            .await
    }

    pub async fn remove_request(self: &Arc<Self>, id: &str) -> AppState {
        {
            let mut persisted = self.persisted.write().await;
            persisted.queue.retain(|item| item.id != id);
        }
        let _ = self.save_persisted().await;
        self.emit_state().await;
        self.ensure_queue_progress("queue removal").await;
        self.snapshot().await
    }

    pub async fn clear_queue(&self) -> AppState {
        {
            let mut persisted = self.persisted.write().await;
            persisted.queue.clear();
        }
        let _ = self.save_persisted().await;
        self.emit_state().await;
        self.snapshot().await
    }

    pub async fn remove_latest_request_by_user(
        self: &Arc<Self>,
        requested_by: &str,
    ) -> CommandResult {
        let removed = {
            let mut persisted = self.persisted.write().await;
            queue_engine::remove_latest_request_by_user(&mut persisted.queue, requested_by)
        };

        match removed {
            Some(_) => {
                let _ = self.save_persisted().await;
                self.emit_state().await;
                self.add_log(
                    LogLevel::Info,
                    format!("Removed the latest request for {requested_by}."),
                )
                .await;
                self.ensure_queue_progress("user remove").await;
                CommandResult::ok("Removed your most recent request.")
            }
            None => CommandResult::error("You do not have any active requests to remove."),
        }
    }

    pub async fn search_apple_music(&self, query: &str) -> Result<SearchResult> {
        let settings = self.current_settings().await;
        let matches = self
            .apple_catalog
            .search_tracks(query, &settings.apple_music)
            .await?;
        Ok(SearchResult {
            query: query.to_string(),
            matches,
        })
    }

    pub async fn open_track(&self, payload: OpenTrackPayload) -> CommandResult {
        let settings = self.current_settings().await;
        if settings.automation.control_mode == AutomationControlMode::StreamerSafe
            && !payload.allow_in_streamer_safe_mode.unwrap_or(false)
        {
            return CommandResult::error(
                "Streamer-safe mode blocks Apple Music launch actions. Use Automation Lab or switch to desktop automation.",
            );
        }

        match self.resolve_track_target(payload).await {
            Ok(target) => match window_shell::open_external(&target) {
                Ok(_) => {
                    self.add_log(
                        LogLevel::Info,
                        format!("Opened Apple Music target: {target}"),
                    )
                    .await;
                    CommandResult::ok("Opened the request in Apple Music.")
                }
                Err(error) => {
                    self.add_log(
                        LogLevel::Error,
                        format!("Open track failed for target {target}: {error}"),
                    )
                    .await;
                    CommandResult::error(error.to_string())
                }
            },
            Err(error) => {
                self.add_log(
                    LogLevel::Error,
                    format!("Open track target resolution failed: {error}"),
                )
                .await;
                CommandResult::error(error.to_string())
            }
        }
    }

    pub async fn run_probe_once(self: &Arc<Self>) -> Result<ProbeResult> {
        let snapshot = self.run_probe_cycle().await?;
        Ok(ProbeResult { snapshot })
    }

    pub async fn run_automation(&self, payload: RunAutomationPayload) -> CommandResult {
        let settings = self.current_settings().await;
        if settings.automation.control_mode == AutomationControlMode::StreamerSafe
            && !payload.allow_in_streamer_safe_mode.unwrap_or(false)
        {
            return CommandResult::error(
                "Streamer-safe mode blocks Apple Music automation. Use Automation Lab or switch to desktop automation.",
            );
        }

        let request = self.find_request(payload.request_id.as_deref()).await;
        let result = self
            .automation_bridge
            .run(&payload, &settings, request.as_ref())
            .await;

        self.record_automation_result(result.clone()).await;
        self.add_log(
            if result.ok {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            format!(
                "Automation {:?} via {:?}: {} ({})",
                result.action, result.adapter, result.summary, result.detail
            ),
        )
        .await;
        self.sync_request_handoff_from_automation(&payload, &result)
            .await;
        if result.ok {
            CommandResult::ok(result.summary)
        } else {
            CommandResult::error(result.detail)
        }
    }

    pub async fn export_diagnostics(&self) -> CommandResult {
        match diagnostics::export_bundle(&self.storage.diagnostics_dir, &self.snapshot().await) {
            Ok(path) => {
                {
                    let mut runtime = self.runtime.write().await;
                    runtime.diagnostics.export_count += 1;
                    runtime.diagnostics.last_export_path = Some(path.display().to_string());
                    runtime.diagnostics.last_summary =
                        "Diagnostics bundle exported with current app state and recent logs."
                            .to_string();
                }
                self.emit_state().await;
                CommandResult::ok(format!("Diagnostics exported to {}.", path.display()))
            }
            Err(error) => CommandResult::error(error.to_string()),
        }
    }

    pub async fn reveal_data_folder(&self) -> CommandResult {
        match window_shell::reveal_directory(&self.storage.data_dir) {
            Ok(_) => CommandResult::ok("Opened the AppleCrap Alpha data folder."),
            Err(error) => CommandResult::error(error.to_string()),
        }
    }

    pub async fn import_legacy_state(self: &Arc<Self>) -> CommandResult {
        match self.storage.import_legacy_state() {
            Ok(Some(legacy_state)) => {
                let adapter = legacy_state.settings.automation.adapter.clone();
                let experimental_enabled = legacy_state
                    .settings
                    .automation
                    .experimental_automation_enabled;
                {
                    let mut persisted = self.persisted.write().await;
                    *persisted = legacy_state;
                }
                {
                    let mut runtime = self.runtime.write().await;
                    runtime.legacy_import.available = false;
                    runtime.legacy_import.imported = true;
                    runtime.legacy_import.message = "Legacy Electron state imported.".to_string();
                    runtime.automation.active_adapter = adapter;
                    runtime.automation.experimental_enabled = experimental_enabled;
                }
                let _ = self.save_persisted().await;
                self.emit_state().await;
                self.ensure_queue_progress("legacy import").await;
                CommandResult::ok("Imported legacy Electron settings, queue, and logs.")
            }
            Ok(None) => CommandResult::error("No legacy Electron state was found to import."),
            Err(error) => CommandResult::error(error.to_string()),
        }
    }

    pub async fn register_twitch_connection(
        &self,
        writer: mpsc::UnboundedSender<String>,
        task: JoinHandle<()>,
    ) {
        let mut connection = self.twitch_connection.lock().await;
        *connection = Some(TwitchConnection { writer, task });
    }

    pub async fn abort_twitch_connection(&self) {
        let mut connection = self.twitch_connection.lock().await;
        if let Some(existing) = connection.take() {
            let _ = existing.writer.send("QUIT :Disconnecting\r\n".to_string());
            existing.task.abort();
        }
    }

    pub async fn update_bot_status(
        &self,
        state: BotConnectionState,
        status: impl Into<String>,
        detail: impl Into<String>,
        channel: Option<String>,
    ) {
        {
            let mut runtime = self.runtime.write().await;
            let connected = matches!(&state, BotConnectionState::Connected);
            let previous_channel = runtime.bot_status.channel.clone();
            runtime.bot_status = BotStatus {
                connected,
                state,
                status: status.into(),
                detail: detail.into(),
                channel: channel.unwrap_or(previous_channel),
                last_event_at: Some(crate::models::now_iso()),
            };
        }
        self.emit_state().await;
    }

    pub async fn clear_twitch_connection(&self) {
        let mut connection = self.twitch_connection.lock().await;
        *connection = None;
    }

    pub async fn run_probe_cycle(self: &Arc<Self>) -> Result<ProbeSnapshot> {
        let top_item = self.persisted.read().await.queue.first().cloned();
        let run = match self.probe_service.run(top_item.as_ref()).await {
            Ok(run) => run,
            Err(error) => {
                let snapshot = ProbeSnapshot {
                    source: "error".to_string(),
                    status: "Unavailable".to_string(),
                    last_error: Some(error.to_string()),
                    explanation: error.to_string(),
                    updated_at: Some(crate::models::now_iso()),
                    ..ProbeSnapshot::default()
                };
                self.set_probe_snapshot(snapshot.clone(), String::new())
                    .await;
                return Ok(snapshot);
            }
        };

        let snapshot = run.snapshot.clone();
        self.set_probe_snapshot(snapshot.clone(), run.session_signature)
            .await;

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

    async fn set_probe_snapshot(&self, snapshot: ProbeSnapshot, session_signature: String) {
        let mut should_log_sessions = None;
        let mut should_log_error = None;

        {
            let mut runtime = self.runtime.write().await;
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

            runtime.probe = snapshot.clone();
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
    }

    async fn record_automation_result(&self, result: AutomationRunResult) {
        {
            let mut runtime = self.runtime.write().await;
            runtime.automation.last_run = Some(result.clone());
            runtime.automation.active_adapter = result.adapter.clone();
        }
        let _ = self.handle.emit("automationSnapshot", result);
        self.emit_state().await;
    }

    async fn sync_request_handoff_from_automation(
        &self,
        payload: &RunAutomationPayload,
        result: &AutomationRunResult,
    ) {
        let Some(request_id) = payload.request_id.as_deref() else {
            return;
        };

        match payload.action {
            crate::models::AutomationAction::AttemptQueueAction
            | crate::models::AutomationAction::AttemptPlay => {
                let state = if result.ok {
                    QueueHandoffState::SentToPlayer
                } else {
                    QueueHandoffState::FailedDispatch
                };
                self.update_request_handoff(
                    request_id,
                    state,
                    Some(if result.ok {
                        result.summary.clone()
                    } else {
                        result.detail.clone()
                    }),
                )
                .await;
            }
            _ => {}
        }
    }

    async fn update_request_handoff(
        &self,
        request_id: &str,
        state: QueueHandoffState,
        note: Option<String>,
    ) {
        let mut changed = false;
        {
            let mut persisted = self.persisted.write().await;
            if let Some(item) = persisted
                .queue
                .iter_mut()
                .find(|item| item.id == request_id)
            {
                item.handoff_state = state;
                item.handoff_note = note.filter(|value| !value.trim().is_empty());
                item.handoff_updated_at = Some(crate::models::now_iso());
                if matches!(item.handoff_state, QueueHandoffState::SentToPlayer) {
                    item.dispatched_at = item.handoff_updated_at.clone();
                }
                changed = true;
            }
        }

        if changed {
            let _ = self.save_persisted().await;
            self.emit_state().await;
        }
    }

    async fn ensure_queue_progress(self: &Arc<Self>, reason: &str) {
        self.promote_front_request(reason).await;

        let (settings, request, action) = {
            let persisted = self.persisted.read().await;
            let settings = persisted.settings.clone();
            let Some(request) = persisted.queue.first().cloned() else {
                return;
            };

            if !settings.automation.auto_arm_enabled
                || !settings.automation.experimental_automation_enabled
                || settings.automation.adapter != crate::models::AutomationAdapterKind::UiAutomation
                || request.resolution != crate::models::ResolutionStatus::Matched
                || request.track.is_none()
                || request.requires_manual_review
                || !matches!(
                    request.handoff_state,
                    QueueHandoffState::PendingMatch | QueueHandoffState::ReadyToSend
                )
            {
                return;
            }

            let action = match settings.automation.handoff_mode {
                crate::models::AutomationHandoffMode::PlayNow => {
                    crate::models::AutomationAction::AttemptPlay
                }
                crate::models::AutomationHandoffMode::PlayNext => {
                    crate::models::AutomationAction::AttemptQueueAction
                }
            };

            (settings, request, action)
        };

        {
            let mut runtime = self.runtime.write().await;
            if runtime.auto_handoff_in_flight {
                return;
            }
            runtime.auto_handoff_in_flight = true;
        }

        let payload = RunAutomationPayload {
            adapter: crate::models::AutomationAdapterKind::UiAutomation,
            action: action.clone(),
            request_id: Some(request.id.clone()),
            dry_run: Some(false),
            allow_in_streamer_safe_mode: Some(
                settings.automation.control_mode == AutomationControlMode::StreamerSafe,
            ),
        };

        let result = self
            .automation_bridge
            .run(&payload, &settings, Some(&request))
            .await;
        self.record_automation_result(result.clone()).await;
        self.add_log(
            if result.ok {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            format!(
                "Auto mode {:?} for \"{}\" after {}: {} ({})",
                result.action,
                request
                    .track
                    .as_ref()
                    .map(|track| track.title.as_str())
                    .unwrap_or(request.query.as_str()),
                reason,
                result.summary,
                result.detail
            ),
        )
        .await;

        let handoff_note = if result.ok {
            Some(result.summary.clone())
        } else {
            Some(result.detail.clone())
        };
        self.update_request_handoff(
            &request.id,
            if result.ok {
                QueueHandoffState::SentToPlayer
            } else {
                QueueHandoffState::FailedDispatch
            },
            handoff_note,
        )
        .await;

        let mut runtime = self.runtime.write().await;
        runtime.auto_handoff_in_flight = false;
    }

    async fn promote_front_request(&self, reason: &str) {
        let settings = self.current_settings().await;
        let mut changed = false;
        {
            let mut persisted = self.persisted.write().await;
            let Some(front) = persisted.queue.first_mut() else {
                return;
            };

            if front.track.is_none() || front.resolution == ResolutionStatus::ManualReview {
                if front.handoff_state != QueueHandoffState::ManualReview
                    || !front.requires_manual_review
                {
                    front.handoff_state = QueueHandoffState::ManualReview;
                    front.requires_manual_review = true;
                    front.handoff_note =
                        Some("Manual review required before this request can be sent.".to_string());
                    front.handoff_updated_at = Some(crate::models::now_iso());
                    changed = true;
                }
            } else if settings.automation.control_mode == AutomationControlMode::StreamerSafe
                && settings.automation.auto_arm_enabled
                && front.handoff_state == QueueHandoffState::PendingMatch
            {
                front.handoff_state = QueueHandoffState::ReadyToSend;
                front.handoff_note =
                    Some(format!("Auto mode prepared this request after {reason}."));
                front.handoff_updated_at = Some(crate::models::now_iso());
                changed = true;
            }
        }

        if changed {
            let _ = self.save_persisted().await;
            self.emit_state().await;
        }
    }

    pub async fn dispatch_ready_request_from_hotkey(self: &Arc<Self>) -> Result<()> {
        let settings = self.current_settings().await;
        if settings.automation.control_mode != AutomationControlMode::StreamerSafe {
            return Ok(());
        }

        self.dispatch_request_inner("global hotkey")
            .await
            .map(|_| ())
    }

    pub async fn dispatch_next_request(self: &Arc<Self>) -> Result<AppState> {
        self.dispatch_request_inner("dashboard dispatch").await?;
        Ok(self.snapshot().await)
    }

    async fn dispatch_request_inner(self: &Arc<Self>, source: &str) -> Result<()> {
        let (settings, request, action) = {
            let persisted = self.persisted.read().await;
            let settings = persisted.settings.clone();
            let Some(request) = persisted.queue.first().cloned() else {
                anyhow::bail!("No request is ready to dispatch.");
            };

            if !settings.automation.experimental_automation_enabled
                || settings.automation.adapter != crate::models::AutomationAdapterKind::UiAutomation
            {
                anyhow::bail!("Streamer-safe dispatch needs the UI automation adapter enabled.");
            }

            if request.track.is_none() || request.requires_manual_review {
                anyhow::bail!("The front request still needs a matched Apple Music track.");
            }

            if !matches!(
                request.handoff_state,
                QueueHandoffState::PendingMatch
                    | QueueHandoffState::ReadyToSend
                    | QueueHandoffState::FailedDispatch
            ) {
                anyhow::bail!("The front request cannot be dispatched right now.");
            }

            let action = match settings.automation.handoff_mode {
                crate::models::AutomationHandoffMode::PlayNow => {
                    crate::models::AutomationAction::AttemptPlay
                }
                crate::models::AutomationHandoffMode::PlayNext => {
                    crate::models::AutomationAction::AttemptQueueAction
                }
            };

            (settings, request, action)
        };

        let payload = RunAutomationPayload {
            adapter: crate::models::AutomationAdapterKind::UiAutomation,
            action: action.clone(),
            request_id: Some(request.id.clone()),
            dry_run: Some(false),
            allow_in_streamer_safe_mode: Some(true),
        };

        let result = self
            .automation_bridge
            .run(&payload, &settings, Some(&request))
            .await;
        self.record_automation_result(result.clone()).await;
        self.add_log(
            if result.ok {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            format!(
                "Streamer-safe dispatch {:?} for \"{}\" via {}: {} ({})",
                result.action,
                request
                    .track
                    .as_ref()
                    .map(|track| track.title.as_str())
                    .unwrap_or(request.query.as_str()),
                source,
                result.summary,
                result.detail
            ),
        )
        .await;

        let handoff_note = if result.ok {
            Some(format!("Triggered via {source}: {}", result.summary))
        } else {
            Some(result.detail.clone())
        };
        self.update_request_handoff(
            &request.id,
            if result.ok {
                QueueHandoffState::SentToPlayer
            } else {
                QueueHandoffState::FailedDispatch
            },
            handoff_note,
        )
        .await;

        if result.ok {
            Ok(())
        } else {
            anyhow::bail!(result.detail)
        }
    }

    pub async fn approve_request(
        self: &Arc<Self>,
        payload: ApproveRequestPayload,
    ) -> Result<AppState> {
        let target_id = if let Some(request_id) = payload.request_id.clone() {
            request_id
        } else {
            self.persisted
                .read()
                .await
                .queue
                .first()
                .map(|item| item.id.clone())
                .ok_or_else(|| anyhow!("No request is available to approve."))?
        };

        {
            let mut persisted = self.persisted.write().await;
            let item = persisted
                .queue
                .iter_mut()
                .find(|item| item.id == target_id)
                .ok_or_else(|| anyhow!("The selected request no longer exists."))?;

            if let Some(track) = payload.track.clone() {
                item.track = Some(track.clone());
                item.resolution = ResolutionStatus::Matched;
                item.resolved_track_url = Some(track.url.clone());
                item.match_confidence = Some(queue_engine::estimate_match_confidence(
                    &item.query,
                    &track.title,
                    &track.artist_name,
                ));
            }

            if item.track.is_none() {
                anyhow::bail!("Approve requires a matched Apple Music track.");
            }

            item.requires_manual_review = false;
            item.handoff_state = QueueHandoffState::ReadyToSend;
            item.handoff_note = Some("Matched and ready to dispatch into Apple Music.".to_string());
            item.handoff_updated_at = Some(crate::models::now_iso());
        }

        self.save_persisted().await?;
        self.emit_state().await;
        self.ensure_queue_progress("request approved").await;
        Ok(self.snapshot().await)
    }

    pub async fn send_request_to_manual_review(self: &Arc<Self>, id: &str) -> Result<AppState> {
        {
            let mut persisted = self.persisted.write().await;
            let item = persisted
                .queue
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow!("The selected request no longer exists."))?;
            item.resolution = ResolutionStatus::ManualReview;
            item.requires_manual_review = true;
            item.handoff_state = QueueHandoffState::ManualReview;
            item.handoff_note =
                Some("Manual review requested before this can be sent.".to_string());
            item.handoff_updated_at = Some(crate::models::now_iso());
        }

        self.save_persisted().await?;
        self.emit_state().await;
        Ok(self.snapshot().await)
    }

    pub async fn set_dispatch_hotkey(self: &Arc<Self>, shortcut: String) -> Result<AppState> {
        self.save_settings(SaveSettingsPayload {
            automation: Some(crate::models::AutomationSettingsPatch {
                dispatch_hotkey: Some(shortcut),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
    }

    async fn apply_dispatch_hotkey(&self, shortcut: &str) -> Result<()> {
        #[cfg(desktop)]
        {
            use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

            let shortcut = Shortcut::try_from(shortcut.trim())
                .map_err(|error| anyhow!("Invalid dispatch hotkey: {error}"))?;
            let manager = self.handle.global_shortcut();
            manager.unregister_all()?;
            manager.register(shortcut)?;
        }

        Ok(())
    }

    async fn resolve_track_target(&self, payload: OpenTrackPayload) -> Result<String> {
        if let Some(url) = payload.url.filter(|value| !value.trim().is_empty()) {
            return Ok(url);
        }

        if let Some(request) = self.find_request(payload.request_id.as_deref()).await {
            if let Some(track) = request.track {
                return Ok(track.url);
            }

            let settings = self.current_settings().await;
            return Ok(AppleCatalog::build_search_url(
                &request.query,
                &settings.apple_music.storefront,
            ));
        }

        if let Some(query) = payload.query.filter(|value| !value.trim().is_empty()) {
            let settings = self.current_settings().await;
            return Ok(AppleCatalog::build_search_url(
                &query,
                &settings.apple_music.storefront,
            ));
        }

        Err(anyhow!("No Apple Music target was available to open."))
    }

    pub async fn find_request(&self, request_id: Option<&str>) -> Option<QueueItem> {
        let persisted = self.persisted.read().await;
        match request_id {
            Some(id) => persisted.queue.iter().find(|item| item.id == id).cloned(),
            None => persisted.queue.first().cloned(),
        }
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
