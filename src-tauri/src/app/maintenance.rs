//! Updates, diagnostics, and legacy data import.

use super::*;

impl AppContext {
    pub async fn check_for_updates(self: &Arc<Self>) -> Result<AppState> {
        let current_version = self.handle.package_info().version.to_string();
        let client = self.http.clone();

        match updater::check_latest(&client, &current_version).await {
            Ok(Some(update)) => {
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

        self.add_log(
            LogLevel::Info,
            format!("Downloading update {}...", update.version),
        )
        .await;

        let client = self.http.clone();
        let staged = match updater::download_and_stage(&client, &update).await
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

        if let Err(error) = updater::swap_and_launch(&staged) {
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
