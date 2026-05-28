use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::models::{
    compact_log_message, AppSettings, LegacyImportStatus, LogEntry, PersistedState,
    QueueHandoffState, QueueItem, StorageInfo, StorageMode, TrackMatch,
};

pub struct SettingsStore {
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub diagnostics_dir: PathBuf,
    pub runtime_log_file: PathBuf,
    pub scripts_dir: PathBuf,
    pub storage: StorageInfo,
}

impl SettingsStore {
    pub fn resolve() -> Result<Self> {
        let executable_dir = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| env::current_dir().ok())
            .context("unable to resolve application directory")?;

        let portable_dir = executable_dir.join("data");
        let (data_dir, mode, warning) = if is_writable_dir(&portable_dir) {
            (portable_dir, StorageMode::Portable, None)
        } else {
            let fallback_base = dirs::data_local_dir().unwrap_or(executable_dir);
            let fallback_dir = fallback_base.join("AppleCrap Alpha");
            fs::create_dir_all(&fallback_dir)?;
            (
                fallback_dir,
                StorageMode::Fallback,
                Some("Portable data folder was not writable, so AppleCrap Alpha fell back to Local AppData.".to_string()),
            )
        };

        let diagnostics_dir = data_dir.join("diagnostics");
        let scripts_dir = data_dir.join("scripts");
        fs::create_dir_all(&diagnostics_dir)?;
        fs::create_dir_all(&scripts_dir)?;

        Ok(Self {
            state_file: data_dir.join("state.json"),
            runtime_log_file: diagnostics_dir.join("runtime.log"),
            storage: StorageInfo {
                mode,
                data_dir: data_dir.display().to_string(),
                warning,
            },
            data_dir,
            diagnostics_dir,
            scripts_dir,
        })
    }

    pub fn write_support_scripts(&self) -> Result<()> {
        fs::create_dir_all(&self.scripts_dir)?;
        fs::write(
            self.scripts_dir.join("now-playing-probe.ps1"),
            include_str!("../scripts/now-playing-probe.ps1"),
        )?;
        fs::write(
            self.scripts_dir.join("apple-music-automation.ps1"),
            include_str!("../scripts/apple-music-automation.ps1"),
        )?;
        Ok(())
    }

    pub fn load_persisted_state(&self) -> PersistedState {
        if !self.state_file.exists() {
            return PersistedState::default();
        }

        match fs::read_to_string(&self.state_file)
            .ok()
            .and_then(|contents| serde_json::from_str::<PersistedState>(&contents).ok())
        {
            Some(mut state) => {
                state.settings.normalize();
                state.logs = sanitize_logs(state.logs, 120);
                state
            }
            None => PersistedState::default(),
        }
    }

    pub fn save_persisted_state(&self, state: &PersistedState) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        fs::write(&self.state_file, serde_json::to_vec_pretty(state)?)?;
        Ok(())
    }

    pub fn append_runtime_log(&self, line: &str) -> Result<()> {
        fs::create_dir_all(&self.diagnostics_dir)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.runtime_log_file)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    pub fn detect_legacy_import(&self) -> LegacyImportStatus {
        let source_path = legacy_candidates().into_iter().find(|path| path.exists());
        LegacyImportStatus {
            available: source_path.is_some(),
            imported: false,
            source_path: source_path.map(|path| path.display().to_string()),
            message: "Import available.".to_string(),
        }
    }

    pub fn import_legacy_state(&self) -> Result<Option<PersistedState>> {
        let source_path = match legacy_candidates().into_iter().find(|path| path.exists()) {
            Some(path) => path,
            None => return Ok(None),
        };

        let contents = fs::read_to_string(&source_path)
            .with_context(|| format!("unable to read legacy state at {}", source_path.display()))?;
        let legacy: LegacyState = serde_json::from_str(&contents).with_context(|| {
            format!("unable to parse legacy state at {}", source_path.display())
        })?;

        let current_settings = AppSettings::default();
        let mut settings = legacy.settings.into_current();
        settings.normalize();
        if settings.twitch.request_command.trim().is_empty() {
            settings.twitch.request_command = current_settings.twitch.request_command;
        }
        if settings.apple_music.storefront.trim().is_empty() {
            settings.apple_music.storefront = current_settings.apple_music.storefront;
        }

        let queue = legacy
            .queue
            .into_iter()
            .map(|item| item.into_current())
            .collect::<Vec<_>>();
        let logs = legacy
            .logs
            .into_iter()
            .take(80)
            .map(|entry| entry.into_current())
            .map(|mut entry| {
                entry.message = compact_log_message(&entry.message);
                entry
            })
            .collect::<Vec<_>>();

        Ok(Some(PersistedState {
            settings,
            queue,
            logs,
        }))
    }
}

fn sanitize_logs(logs: Vec<LogEntry>, max_entries: usize) -> Vec<LogEntry> {
    logs.into_iter()
        .take(max_entries)
        .map(|mut entry| {
            entry.message = compact_log_message(&entry.message);
            entry
        })
        .collect()
}

fn is_writable_dir(path: &Path) -> bool {
    if fs::create_dir_all(path).is_err() {
        return false;
    }

    let test_file = path.join(".applecrap-write-check");
    let writable = fs::write(&test_file, b"ok").is_ok();
    let _ = fs::remove_file(test_file);
    writable
}

fn legacy_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(appdata) = env::var("APPDATA") {
        candidates.push(
            PathBuf::from(&appdata)
                .join("AppleCrap")
                .join("song-requests.json"),
        );
        candidates.push(
            PathBuf::from(&appdata)
                .join("applecrap")
                .join("song-requests.json"),
        );
    }

    if let Ok(local_appdata) = env::var("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(&local_appdata)
                .join("AppleCrap")
                .join("song-requests.json"),
        );
    }

    candidates
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyState {
    settings: LegacySettings,
    queue: Vec<LegacyQueueItem>,
    logs: Vec<LegacyLogEntry>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacySettings {
    twitch: LegacyTwitchSettings,
    request_limits: LegacyRequestLimits,
    apple_music: LegacyAppleMusicSettings,
}

impl LegacySettings {
    fn into_current(self) -> AppSettings {
        AppSettings {
            twitch: self.twitch.into_current(),
            request_limits: self.request_limits.into_current(),
            apple_music: self.apple_music.into_current(),
            automation: Default::default(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyTwitchSettings {
    channel: String,
    bot_username: String,
    oauth_token: String,
    request_command: String,
    auto_connect: bool,
}

impl LegacyTwitchSettings {
    fn into_current(self) -> crate::models::TwitchSettings {
        crate::models::TwitchSettings {
            channel: self.channel,
            bot_username: self.bot_username,
            oauth_token: self.oauth_token,
            request_command: self.request_command,
            auto_connect: self.auto_connect,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyRequestLimits {
    max_queue_size: u32,
    max_per_user: u32,
    cooldown_seconds: u32,
    allow_duplicates: bool,
    allow_links: bool,
    mods_bypass_limits: bool,
    max_track_minutes: u32,
}

impl LegacyRequestLimits {
    fn into_current(self) -> crate::models::RequestLimits {
        crate::models::RequestLimits {
            max_queue_size: if self.max_queue_size == 0 {
                25
            } else {
                self.max_queue_size
            },
            max_per_user: if self.max_per_user == 0 {
                2
            } else {
                self.max_per_user
            },
            cooldown_seconds: self.cooldown_seconds,
            allow_duplicates: self.allow_duplicates,
            allow_links: self.allow_links,
            mods_bypass_limits: self.mods_bypass_limits,
            max_track_minutes: if self.max_track_minutes == 0 {
                10
            } else {
                self.max_track_minutes
            },
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyAppleMusicSettings {
    storefront: String,
}

impl LegacyAppleMusicSettings {
    fn into_current(self) -> crate::models::AppleMusicSettings {
        crate::models::AppleMusicSettings {
            storefront: self.storefront,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyQueueItem {
    id: String,
    requested_by: String,
    query: String,
    submitted_at: String,
    source: String,
    resolution: crate::models::ResolutionStatus,
    track: Option<TrackMatch>,
}

impl LegacyQueueItem {
    fn into_current(self) -> QueueItem {
        let track = self.track;
        QueueItem {
            id: self.id,
            requested_by: self.requested_by,
            query: self.query,
            submitted_at: self.submitted_at,
            source: self.source,
            resolution: self.resolution,
            resolved_track_url: track.as_ref().map(|track| track.url.clone()),
            match_confidence: None,
            requires_manual_review: track.is_none(),
            handoff_state: if track.is_some() {
                QueueHandoffState::PendingMatch
            } else {
                QueueHandoffState::ManualReview
            },
            track,
            handoff_note: None,
            handoff_updated_at: None,
            dispatched_at: None,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyLogEntry {
    id: String,
    level: crate::models::LogLevel,
    message: String,
    timestamp: String,
}

impl LegacyLogEntry {
    fn into_current(self) -> LogEntry {
        LogEntry {
            id: self.id,
            level: self.level,
            message: self.message,
            timestamp: self.timestamp,
        }
    }
}
