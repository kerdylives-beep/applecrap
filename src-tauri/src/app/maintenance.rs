//! Updates, diagnostics, and legacy data import.

use crate::{
    models::{Alert, AlertAction, AlertTone},
    services::update_guard,
};

use super::*;

impl AppContext {
    pub async fn check_for_updates(self: &Arc<Self>) -> Result<AppState> {
        let current_version = self.handle.package_info().version.to_string();
        let client = self.http.clone();

        match updater::check_latest(&client, &current_version).await {
            Ok(Some(mut update)) => {
                update.rolled_back = self.rolled_back_version().as_deref()
                    == Some(update.version.trim_start_matches('v'));
                self.add_log(
                    LogLevel::Info,
                    format!(
                        "Update {} is available (running {current_version}).",
                        update.version
                    ),
                )
                .await;
                let mut runtime = self.runtime.write().await;
                runtime.update = Some(update);
            }
            Ok(None) => {
                let mut runtime = self.runtime.write().await;
                runtime.update = None;
            }
            Err(error) => {
                self.add_log(LogLevel::Warn, format!("Update check failed: {error}"))
                    .await;
            }
        }

        self.emit_state().await;
        Ok(self.snapshot().await)
    }

    pub async fn install_update(self: &Arc<Self>) -> CommandResult {
        let Some(update) = self.runtime.read().await.update.clone() else {
            return CommandResult::error("No update is available to install.");
        };
        if update.rolled_back {
            return CommandResult::error(format!(
                "Version {} didn't start on this PC last time, so it won't install automatically. You can download it from {}.",
                update.version, update.release_url
            ));
        }

        self.add_log(
            LogLevel::Info,
            format!("Downloading update {}...", update.version),
        )
        .await;

        let client = self.http.clone();
        let _staged = match updater::download_and_stage(&client, &update).await
        {
            Ok(staged) => staged,
            Err(error) => {
                self.add_log(LogLevel::Error, format!("Update download failed: {error}"))
                    .await;
                return CommandResult::error(format!(
                    "Update failed: {error}. You can download it manually from {}.",
                    update.release_url
                ));
            }
        };

        self.add_log(
            LogLevel::Info,
            format!(
                "Update {} downloaded. Restarting into the new version. Settings and queue are kept.",
                update.version
            ),
        )
        .await;

        let installed = update_guard::Layout::current().and_then(|layout| {
            update_guard::install(
                &layout,
                &self.handle.package_info().version.to_string(),
                update.version.trim_start_matches('v'),
            )
        });
        if let Err(error) = installed {
            self.add_log(LogLevel::Error, format!("Update install failed: {error}"))
                .await;
            return CommandResult::error(format!(
                "Update failed: {error}. You can download it manually from {}.",
                update.release_url
            ));
        }

        let handle = self.handle.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            handle.exit(0);
        });

        CommandResult::ok("Update installed. Restarting...")
    }

    /// The UI loaded: if this run is a fresh update on probation, it passed.
    pub fn note_ui_loaded(&self) {
        if let Ok(layout) = update_guard::Layout::current() {
            update_guard::mark_healthy(&layout, &self.handle.package_info().version.to_string());
        }
    }

    fn rolled_back_version(&self) -> Option<String> {
        update_guard::Layout::current()
            .ok()
            .and_then(|layout| update_guard::rolled_back_version(&layout))
    }

    /// Tells the user an update was undone. Runs during setup, before any
    /// UI exists, so it only records the alert.
    pub fn report_update_notice(self: &Arc<Self>, notice: update_guard::StartupNotice) {
        let update_guard::StartupNotice::RolledBack { failed } = notice;
        let current = self.handle.package_info().version.to_string();
        if let Ok(mut runtime) = self.runtime.try_write() {
            runtime.alerts.push(Alert {
                id: "update-rolled-back".to_string(),
                tone: AlertTone::Warn,
                title: format!("Update {failed} didn't start, so AppleCrap went back to {current}"),
                detail: "Nothing was lost. It won't try that version again; the next update installs as usual.".to_string(),
                action: Some(AlertAction::ReportProblem),
            });
        }
        let context = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            context
                .add_log(LogLevel::Warn, format!("Update {failed} didn't start; rolled back to {current}."))
                .await;
        });
    }

    /// Shows a banner, replacing any earlier one with the same id.
    pub async fn raise_alert(&self, alert: Alert) {
        {
            let mut runtime = self.runtime.write().await;
            if runtime.alerts.iter().any(|existing| *existing == alert) {
                return;
            }
            runtime.alerts.retain(|existing| existing.id != alert.id);
            runtime.alerts.push(alert);
        }
        self.emit_state().await;
    }

    pub async fn clear_alert(&self, id: &str) {
        let removed = {
            let mut runtime = self.runtime.write().await;
            let before = runtime.alerts.len();
            runtime.alerts.retain(|alert| alert.id != id);
            before != runtime.alerts.len()
        };
        if removed {
            self.emit_state().await;
        }
    }

    pub async fn dismiss_alert(&self, id: &str) -> AppState {
        self.clear_alert(id).await;
        self.snapshot().await
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

    /// Saves a diagnostics bundle and shows it in Explorer, ready to attach
    /// to the email the UI opens next.
    pub async fn report_problem(&self) -> CommandResult {
        match diagnostics::export_bundle(&self.storage.diagnostics_dir, &self.snapshot().await) {
            Ok(path) => {
                let _ = window_shell::reveal_file(&path);
                self.add_log(
                    LogLevel::Info,
                    format!("Saved a problem report to {}.", path.display()),
                )
                .await;
                CommandResult::ok(path.display().to_string())
            }
            Err(error) => CommandResult::error(format!("Couldn't save the report: {error}")),
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
                {
                    let mut persisted = self.persisted.write().await;
                    *persisted = legacy_state;
                }
                {
                    let mut runtime = self.runtime.write().await;
                    runtime.legacy_import.available = false;
                    runtime.legacy_import.imported = true;
                    runtime.legacy_import.message = "Legacy Electron state imported.".to_string();
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
}
