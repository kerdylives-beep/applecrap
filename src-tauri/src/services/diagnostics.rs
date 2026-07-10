use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::Result;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::models::{AppState, LogEntry};

pub fn export_bundle(diagnostics_dir: &Path, snapshot: &AppState) -> Result<PathBuf> {
    let safe_snapshot = redact_snapshot(snapshot);
    std::fs::create_dir_all(diagnostics_dir)?;
    let file_name = format!(
        "applecrap-alpha-diagnostics-{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let output_path = diagnostics_dir.join(file_name);
    let file = File::create(&output_path)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    archive.start_file("state.json", options)?;
    archive.write_all(serde_json::to_string_pretty(&safe_snapshot)?.as_bytes())?;

    archive.start_file("summary.txt", options)?;
    archive.write_all(build_summary(&safe_snapshot).as_bytes())?;

    archive.start_file("logs.txt", options)?;
    let log_text = safe_snapshot
        .logs
        .iter()
        .map(|entry| {
            format!(
                "[{}] {} {}",
                entry.level_string(),
                entry.timestamp,
                entry.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    archive.write_all(log_text.as_bytes())?;

    let runtime_log_path = diagnostics_dir.join("runtime.log");
    if runtime_log_path.exists() {
        archive.start_file("runtime.log", options)?;
        let runtime_log = std::fs::read_to_string(runtime_log_path)?;
        archive.write_all(redact_sensitive_text(&runtime_log).as_bytes())?;
    }

    archive.finish()?;
    Ok(output_path)
}

fn redact_snapshot(snapshot: &AppState) -> AppState {
    let mut safe_snapshot = snapshot.clone();
    if !safe_snapshot.settings.twitch.oauth_token.is_empty() {
        safe_snapshot.settings.twitch.oauth_token = "<redacted>".to_string();
    }
    safe_snapshot.logs = safe_snapshot
        .logs
        .into_iter()
        .map(redact_log_entry)
        .collect();
    safe_snapshot
}

fn redact_log_entry(mut entry: LogEntry) -> LogEntry {
    entry.message = redact_sensitive_text(&entry.message);
    entry
}

fn redact_sensitive_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            if part.to_ascii_lowercase().starts_with("oauth:") {
                "oauth:<redacted>"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_summary(snapshot: &AppState) -> String {
    [
        format!(
            "Storage: {} ({})",
            snapshot.storage.mode_string(),
            snapshot.storage.data_dir
        ),
        format!("Bot: {}", snapshot.bot_status.status),
        format!("Queue: {} item(s)", snapshot.stats.total_requests),
        format!("Probe: {}", snapshot.probe.status),
        String::new(),
        snapshot.diagnostics.last_summary.clone(),
    ]
    .join("\n")
}

trait DiagnosticsFormat {
    fn level_string(&self) -> &'static str;
}

impl DiagnosticsFormat for crate::models::LogEntry {
    fn level_string(&self) -> &'static str {
        match self.level {
            crate::models::LogLevel::Info => "INFO",
            crate::models::LogLevel::Warn => "WARN",
            crate::models::LogLevel::Error => "ERROR",
            crate::models::LogLevel::Debug => "DEBUG",
        }
    }
}

trait StorageModeFormat {
    fn mode_string(&self) -> &'static str;
}

impl StorageModeFormat for crate::models::StorageInfo {
    fn mode_string(&self) -> &'static str {
        match self.mode {
            crate::models::StorageMode::Portable => "portable",
            crate::models::StorageMode::Fallback => "fallback",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{redact_sensitive_text, redact_snapshot};
    use crate::models::{AppState, LogEntry, LogLevel};

    #[test]
    fn redacts_oauth_token_from_diagnostics_snapshot() {
        let mut snapshot = AppState::default();
        snapshot.settings.twitch.oauth_token = "oauth:test-token".to_string();

        let safe_snapshot = redact_snapshot(&snapshot);

        assert_eq!(safe_snapshot.settings.twitch.oauth_token, "<redacted>");
    }

    #[test]
    fn redacts_oauth_token_from_logs() {
        let mut snapshot = AppState::default();
        snapshot.logs.push(LogEntry {
            level: LogLevel::Info,
            message: "token oauth:test-token connected".to_string(),
            ..Default::default()
        });

        let safe_snapshot = redact_snapshot(&snapshot);

        assert_eq!(
            safe_snapshot.logs[0].message,
            "token oauth:<redacted> connected"
        );
        assert_eq!(
            redact_sensitive_text("failed with OAUTH:test-token"),
            "failed with oauth:<redacted>"
        );
    }
}
