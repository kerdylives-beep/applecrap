use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{
    models::{
        compact_log_message, AppSettings, LegacyImportStatus, LogEntry, PersistedState,
        QueueHandoffState, QueueItem, StorageInfo, StorageMode, TrackMatch,
    },
    services::secret_store,
};

pub struct SettingsStore {
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub diagnostics_dir: PathBuf,
    pub runtime_log_file: PathBuf,
    pub storage: StorageInfo,
    log_handle: Mutex<Option<fs::File>>,
}

/// Where persisted state lives, detached from the store so a save can move
/// onto a blocking thread.
pub struct StateTarget {
    data_dir: PathBuf,
    state_file: PathBuf,
}

impl StateTarget {
    /// Writes state atomically: a temp file is written and flushed to disk,
    /// then renamed over the real file (an atomic replace on NTFS). A crash
    /// mid-save leaves either the old file or the new one — never a truncated
    /// mix, which previously wiped every setting on the next launch.
    pub fn save(&self, state: &PersistedState) -> Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        let mut on_disk = state.clone();
        protect_secrets(&mut on_disk);
        let bytes = serde_json::to_vec_pretty(&on_disk)?;
        let temp = self.state_file.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&temp, &self.state_file)?;
        Ok(())
    }
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
                Some("The folder next to the app isn't writable, so AppleCrap keeps its data in Local AppData instead.".to_string()),
            )
        };

        let diagnostics_dir = data_dir.join("diagnostics");
        fs::create_dir_all(&diagnostics_dir)?;

        let store = Self {
            state_file: data_dir.join("state.json"),
            runtime_log_file: diagnostics_dir.join("runtime.log"),
            storage: StorageInfo {
                mode,
                data_dir: data_dir.display().to_string(),
                warning,
            },
            data_dir,
            diagnostics_dir,
            log_handle: Mutex::new(None),
        };
        store.rotate_runtime_log();
        Ok(store)
    }

    fn backup_file(&self) -> PathBuf {
        self.state_file.with_extension("json.bak")
    }

    /// Loads persisted state, recovering from a damaged file instead of
    /// silently resetting.
    ///
    /// A damaged `state.json` is moved aside (never overwritten, so it can
    /// still be recovered by hand), the last known-good backup is tried, and
    /// the outcome is surfaced through `storage.warning` so the user is told
    /// rather than finding their bot setup quietly gone.
    pub fn load_persisted_state(&mut self) -> PersistedState {
        if !self.state_file.exists() {
            return PersistedState::default();
        }

        if let Some((state, secrets_readable)) = read_state_file(&self.state_file) {
            // Known-good: refresh the backup used for recovery next time.
            let _ = fs::copy(&self.state_file, self.backup_file());
            if !secrets_readable {
                self.add_warning(
                    "Your saved Twitch sign-in could not be read on this Windows account (the folder may have been moved to another PC). Please sign in again.",
                );
            }
            return state;
        }

        let preserved = self.data_dir.join(format!(
            "state.corrupt-{}.json",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let preserved_note = match fs::rename(&self.state_file, &preserved) {
            Ok(()) => format!(" The damaged file was kept as {}.", preserved.display()),
            Err(_) => String::new(),
        };

        let (state, message) = match read_state_file(&self.backup_file()) {
            Some((state, _)) => (
                state,
                format!("Your settings file was damaged, so AppleCrap restored the last good backup.{preserved_note}"),
            ),
            None => (
                PersistedState::default(),
                format!("Your settings file was damaged and no backup was available, so AppleCrap started with default settings.{preserved_note}"),
            ),
        };

        self.add_warning(&message);
        state
    }

    fn add_warning(&mut self, message: &str) {
        self.storage.warning = Some(match self.storage.warning.take() {
            Some(existing) => format!("{existing} {message}"),
            None => message.to_string(),
        });
    }

    pub fn save_persisted_state(&self, state: &PersistedState) -> Result<()> {
        self.state_target().save(state)
    }

    /// An owned handle to where state is saved, so a save can run on a
    /// blocking thread instead of stalling the async runtime.
    pub fn state_target(&self) -> StateTarget {
        StateTarget {
            data_dir: self.data_dir.clone(),
            state_file: self.state_file.clone(),
        }
    }

    /// Appends one line to runtime.log. The file handle is opened once and
    /// kept: opening a file per line is slow on Windows (antivirus scans every
    /// open) and this runs for every log entry.
    pub fn append_runtime_log(&self, line: &str) -> Result<()> {
        let mut handle = self
            .log_handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if handle.is_none() {
            fs::create_dir_all(&self.diagnostics_dir)?;
            *handle = Some(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.runtime_log_file)?,
            );
        }
        let result = writeln!(handle.as_mut().expect("opened above"), "{line}");
        if result.is_err() {
            // Reopen on the next line rather than failing forever.
            *handle = None;
        }
        Ok(result?)
    }

    /// Keeps runtime.log bounded. It is appended to for as long as the app
    /// runs and was never trimmed, so it grew without limit across streams.
    fn rotate_runtime_log(&self) {
        const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
        let too_big = fs::metadata(&self.runtime_log_file)
            .map(|meta| meta.len() > MAX_LOG_BYTES)
            .unwrap_or(false);
        if too_big {
            let previous = self.runtime_log_file.with_extension("log.1");
            let _ = fs::remove_file(&previous);
            let _ = fs::rename(&self.runtime_log_file, previous);
        }
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
            auth: Default::default(),
            channel_points: Default::default(),
        }))
    }
}

/// Parses a state file, returning `None` if it is missing or unreadable.
/// The flag reports whether every saved credential could be decrypted.
fn read_state_file(path: &Path) -> Option<(PersistedState, bool)> {
    let contents = fs::read_to_string(path).ok()?;
    let mut state = serde_json::from_str::<PersistedState>(&contents).ok()?;
    let secrets_readable = reveal_secrets(&mut state);
    state.settings.normalize();
    state.logs = sanitize_logs(state.logs, 120);
    Some((state, secrets_readable))
}

/// The single list of credential fields encrypted at rest. Anything that
/// stores a token must be added here and in `reveal_secrets`.
fn protect_secrets(state: &mut PersistedState) {
    let token = &mut state.settings.twitch.oauth_token;
    *token = secret_store::protect(token);
    for account in [&mut state.auth.bot, &mut state.auth.broadcaster]
        .into_iter()
        .flatten()
    {
        account.access_token = secret_store::protect(&account.access_token);
        account.refresh_token = secret_store::protect(&account.refresh_token);
    }
}

/// Decrypts credential fields in place. A field that cannot be decrypted is
/// cleared (the user signs in again) and reported via the return value.
fn reveal_secrets(state: &mut PersistedState) -> bool {
    let mut all_readable = true;
    let token = &mut state.settings.twitch.oauth_token;
    match secret_store::unprotect(token) {
        Ok(plain) => *token = plain,
        Err(_) => {
            token.clear();
            all_readable = false;
        }
    }

    for account in [&mut state.auth.bot, &mut state.auth.broadcaster] {
        let Some(signed_in) = account.as_mut() else {
            continue;
        };
        let access = secret_store::unprotect(&signed_in.access_token);
        let refresh = secret_store::unprotect(&signed_in.refresh_token);
        match (access, refresh) {
            (Ok(access), Ok(refresh)) => {
                signed_in.access_token = access;
                signed_in.refresh_token = refresh;
            }
            // Half a credential is useless; drop the account so the UI asks
            // for a fresh sign-in instead of failing on every request.
            _ => {
                *account = None;
                all_readable = false;
            }
        }
    }
    all_readable
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
            player: Default::default(),
            overlay: Default::default(),
            channel_points: Default::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> SettingsStore {
        let dir = std::env::temp_dir().join(format!(
            "applecrap-store-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        SettingsStore {
            state_file: dir.join("state.json"),
            runtime_log_file: dir.join("runtime.log"),
            diagnostics_dir: dir.clone(),
            storage: StorageInfo::default(),
            data_dir: dir,
            log_handle: Mutex::new(None),
        }
    }

    fn configured_state() -> PersistedState {
        let mut state = PersistedState::default();
        state.settings.twitch.channel = "kerdylives".to_string();
        state.settings.twitch.bot_username = "kerdyknives".to_string();
        state
    }

    #[test]
    fn save_is_atomic_and_round_trips() {
        let mut store = temp_store("roundtrip");
        store.save_persisted_state(&configured_state()).unwrap();

        assert!(!store.state_file.with_extension("json.tmp").exists());
        let loaded = store.load_persisted_state();
        assert_eq!(loaded.settings.twitch.channel, "kerdylives");
        assert!(store.storage.warning.is_none());
    }

    // Regression: a truncated state.json used to silently reset every setting
    // and was then overwritten with defaults, destroying any chance of recovery.
    #[test]
    fn damaged_state_recovers_from_backup_and_is_preserved() {
        let mut store = temp_store("recover");
        store.save_persisted_state(&configured_state()).unwrap();
        // A clean load refreshes the backup.
        let _ = store.load_persisted_state();

        fs::write(&store.state_file, b"{\"settings\": {\"twitch\": ").unwrap();
        let loaded = store.load_persisted_state();

        assert_eq!(loaded.settings.twitch.channel, "kerdylives");
        assert!(store.storage.warning.as_deref().unwrap().contains("restored the last good backup"));
        let preserved = fs::read_dir(&store.data_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().starts_with("state.corrupt-"));
        assert!(preserved, "damaged file must be kept for manual recovery");
    }

    #[cfg(windows)]
    #[test]
    fn token_is_encrypted_on_disk_and_restored_on_load() {
        let mut store = temp_store("secrets");
        let mut state = configured_state();
        state.settings.twitch.oauth_token = "oauth:supersecret".to_string();
        store.save_persisted_state(&state).unwrap();

        let raw = fs::read_to_string(&store.state_file).unwrap();
        assert!(!raw.contains("supersecret"), "token must not be stored in plain text");

        let loaded = store.load_persisted_state();
        assert_eq!(loaded.settings.twitch.oauth_token, "oauth:supersecret");
    }

    #[test]
    fn damaged_state_without_backup_warns_instead_of_silently_resetting() {
        let mut store = temp_store("nobackup");
        fs::write(&store.state_file, b"not json at all").unwrap();

        let loaded = store.load_persisted_state();

        assert!(loaded.settings.twitch.channel.is_empty());
        assert!(store.storage.warning.as_deref().unwrap().contains("no backup was available"));
        assert!(!store.state_file.exists(), "damaged file must be moved aside, not left to be overwritten");
    }
}
