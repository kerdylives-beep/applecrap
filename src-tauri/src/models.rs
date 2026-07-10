use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionStatus {
    Matched,
    ManualReview,
}

impl Default for ResolutionStatus {
    fn default() -> Self {
        Self::ManualReview
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Info
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    Portable,
    Fallback,
}

impl Default for StorageMode {
    fn default() -> Self {
        Self::Portable
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BotConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

impl Default for BotConnectionState {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AutomationHandoffMode {
    PlayNow,
    PlayNext,
}

impl Default for AutomationHandoffMode {
    fn default() -> Self {
        Self::PlayNext
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AutomationControlMode {
    StreamerSafe,
    DesktopAutomation,
}

impl Default for AutomationControlMode {
    fn default() -> Self {
        Self::StreamerSafe
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QueueHandoffState {
    PendingMatch,
    ReadyToSend,
    SentToPlayer,
    ConfirmedPlaying,
    ManualReview,
    FailedDispatch,
}

impl Default for QueueHandoffState {
    fn default() -> Self {
        Self::PendingMatch
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackMatch {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration_ms: Option<i64>,
    pub url: String,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct QueueItem {
    pub id: String,
    pub requested_by: String,
    pub query: String,
    pub submitted_at: String,
    pub source: String,
    pub resolution: ResolutionStatus,
    pub track: Option<TrackMatch>,
    pub handoff_state: QueueHandoffState,
    pub resolved_track_url: Option<String>,
    pub match_confidence: Option<f32>,
    pub requires_manual_review: bool,
    pub handoff_note: Option<String>,
    pub handoff_updated_at: Option<String>,
    pub dispatched_at: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TwitchSettings {
    pub channel: String,
    pub bot_username: String,
    pub oauth_token: String,
    pub request_command: String,
    pub auto_connect: bool,
}

impl Default for TwitchSettings {
    fn default() -> Self {
        Self {
            channel: String::new(),
            bot_username: String::new(),
            oauth_token: String::new(),
            request_command: "!request".to_string(),
            auto_connect: false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RequestLimits {
    pub max_queue_size: u32,
    pub max_per_user: u32,
    pub cooldown_seconds: u32,
    pub allow_duplicates: bool,
    pub allow_links: bool,
    pub mods_bypass_limits: bool,
    pub max_track_minutes: u32,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            max_queue_size: 25,
            max_per_user: 2,
            cooldown_seconds: 120,
            allow_duplicates: false,
            allow_links: true,
            mods_bypass_limits: true,
            max_track_minutes: 10,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppleMusicSettings {
    pub storefront: String,
}

impl Default for AppleMusicSettings {
    fn default() -> Self {
        Self {
            storefront: "us".to_string(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct AutomationSettings {
    #[serde(alias = "controlMode")]
    pub control_mode: AutomationControlMode,
    pub experimental_automation_enabled: bool,
    pub handoff_mode: AutomationHandoffMode,
    #[serde(default = "default_dispatch_hotkey", alias = "dispatchHotkey")]
    pub dispatch_hotkey: String,
    #[serde(alias = "autoModeEnabled")]
    pub auto_arm_enabled: bool,
}

impl Default for AutomationSettings {
    fn default() -> Self {
        Self {
            control_mode: AutomationControlMode::StreamerSafe,
            experimental_automation_enabled: true,
            handoff_mode: AutomationHandoffMode::PlayNext,
            dispatch_hotkey: default_dispatch_hotkey(),
            auto_arm_enabled: false,
        }
    }
}

fn default_dispatch_hotkey() -> String {
    "F8".to_string()
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub twitch: TwitchSettings,
    pub request_limits: RequestLimits,
    pub apple_music: AppleMusicSettings,
    pub automation: AutomationSettings,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BotStatus {
    pub connected: bool,
    pub state: BotConnectionState,
    pub status: String,
    pub detail: String,
    pub channel: String,
    pub last_event_at: Option<String>,
}

impl Default for BotStatus {
    fn default() -> Self {
        Self {
            connected: false,
            state: BotConnectionState::Disconnected,
            status: "Disconnected".to_string(),
            detail: "Bot is offline.".to_string(),
            channel: String::new(),
            last_event_at: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    pub level: LogLevel,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSession {
    pub app_id: String,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSnapshot {
    pub source: String,
    pub app_id: String,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub matched_queue_id: Option<String>,
    pub matched: bool,
    pub confidence: f32,
    pub explanation: String,
    pub last_error: Option<String>,
    pub sessions: Vec<ProbeSession>,
    pub updated_at: Option<String>,
}

impl Default for ProbeSnapshot {
    fn default() -> Self {
        Self {
            source: "idle".to_string(),
            app_id: String::new(),
            status: "Stopped".to_string(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            matched_queue_id: None,
            matched: false,
            confidence: 0.0,
            explanation: "Waiting for a playback probe.".to_string(),
            last_error: None,
            sessions: Vec::new(),
            updated_at: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub last_export_path: Option<String>,
    pub export_count: u32,
    pub last_summary: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LegacyImportStatus {
    pub available: bool,
    pub imported: bool,
    pub source_path: Option<String>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub mode: StorageMode,
    pub data_dir: String,
    pub warning: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppStats {
    pub total_requests: usize,
    pub unresolved_requests: usize,
    pub matched_requests: usize,
    pub connected_since: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub settings: AppSettings,
    pub queue: Vec<QueueItem>,
    pub logs: Vec<LogEntry>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub settings: AppSettings,
    pub queue: Vec<QueueItem>,
    pub ready_request: Option<QueueItem>,
    pub control_mode: AutomationControlMode,
    pub logs: Vec<LogEntry>,
    pub bot_status: BotStatus,
    pub probe: ProbeSnapshot,
    pub diagnostics: DiagnosticsSnapshot,
    pub legacy_import: LegacyImportStatus,
    pub storage: StorageInfo,
    pub stats: AppStats,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TwitchSettingsPatch {
    pub channel: Option<String>,
    pub bot_username: Option<String>,
    pub oauth_token: Option<String>,
    pub request_command: Option<String>,
    pub auto_connect: Option<bool>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RequestLimitsPatch {
    pub max_queue_size: Option<u32>,
    pub max_per_user: Option<u32>,
    pub cooldown_seconds: Option<u32>,
    pub allow_duplicates: Option<bool>,
    pub allow_links: Option<bool>,
    pub mods_bypass_limits: Option<bool>,
    pub max_track_minutes: Option<u32>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppleMusicSettingsPatch {
    pub storefront: Option<String>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSettingsPatch {
    pub control_mode: Option<AutomationControlMode>,
    pub experimental_automation_enabled: Option<bool>,
    pub handoff_mode: Option<AutomationHandoffMode>,
    pub dispatch_hotkey: Option<String>,
    #[serde(alias = "autoModeEnabled")]
    pub auto_arm_enabled: Option<bool>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsPayload {
    pub twitch: Option<TwitchSettingsPatch>,
    pub request_limits: Option<RequestLimitsPatch>,
    pub apple_music: Option<AppleMusicSettingsPatch>,
    pub automation: Option<AutomationSettingsPatch>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManualRequestPayload {
    pub requested_by: String,
    pub query: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub query: String,
    pub matches: Vec<TrackMatch>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub snapshot: ProbeSnapshot,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpenTrackPayload {
    pub request_id: Option<String>,
    pub url: Option<String>,
    pub query: Option<String>,
    pub allow_in_streamer_safe_mode: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApproveRequestPayload {
    pub request_id: Option<String>,
    pub track: Option<TrackMatch>,
}

impl AppSettings {
    pub fn merge_patch(&mut self, patch: SaveSettingsPayload) {
        if let Some(twitch) = patch.twitch {
            if let Some(channel) = twitch.channel {
                self.twitch.channel = channel.trim().to_string();
            }
            if let Some(bot_username) = twitch.bot_username {
                self.twitch.bot_username = bot_username.trim().to_string();
            }
            if let Some(oauth_token) = twitch.oauth_token {
                self.twitch.oauth_token = normalize_twitch_oauth_token(&oauth_token);
            }
            if let Some(request_command) = twitch.request_command {
                self.twitch.request_command = if request_command.trim().is_empty() {
                    "!request".to_string()
                } else {
                    request_command.trim().to_string()
                };
            }
            if let Some(auto_connect) = twitch.auto_connect {
                self.twitch.auto_connect = auto_connect;
            }
        }

        if let Some(request_limits) = patch.request_limits {
            if let Some(value) = request_limits.max_queue_size {
                self.request_limits.max_queue_size = value.clamp(1, 500);
            }
            if let Some(value) = request_limits.max_per_user {
                self.request_limits.max_per_user = value.clamp(1, 20);
            }
            if let Some(value) = request_limits.cooldown_seconds {
                self.request_limits.cooldown_seconds = value.clamp(0, 3600);
            }
            if let Some(value) = request_limits.allow_duplicates {
                self.request_limits.allow_duplicates = value;
            }
            if let Some(value) = request_limits.allow_links {
                self.request_limits.allow_links = value;
            }
            if let Some(value) = request_limits.mods_bypass_limits {
                self.request_limits.mods_bypass_limits = value;
            }
            if let Some(value) = request_limits.max_track_minutes {
                self.request_limits.max_track_minutes = value.clamp(1, 30);
            }
        }

        if let Some(apple_music) = patch.apple_music {
            if let Some(storefront) = apple_music.storefront {
                self.apple_music.storefront = if storefront.trim().is_empty() {
                    "us".to_string()
                } else {
                    storefront.trim().to_lowercase()
                };
            }
        }

        if let Some(automation) = patch.automation {
            if let Some(control_mode) = automation.control_mode {
                self.automation.control_mode = control_mode;
            }
            if let Some(enabled) = automation.experimental_automation_enabled {
                self.automation.experimental_automation_enabled = enabled;
            }
            if let Some(handoff_mode) = automation.handoff_mode {
                self.automation.handoff_mode = handoff_mode;
            }
            if let Some(dispatch_hotkey) = automation.dispatch_hotkey {
                self.automation.dispatch_hotkey = dispatch_hotkey.trim().to_string();
            }
            if let Some(auto_arm_enabled) = automation.auto_arm_enabled {
                self.automation.auto_arm_enabled = auto_arm_enabled;
            }
        }

        self.normalize();
    }

    pub fn normalize(&mut self) {
        self.twitch.channel = self
            .twitch
            .channel
            .trim()
            .trim_start_matches('#')
            .to_string();
        self.twitch.bot_username = self.twitch.bot_username.trim().to_string();
        self.twitch.oauth_token = normalize_twitch_oauth_token(&self.twitch.oauth_token);
        self.twitch.request_command = if self.twitch.request_command.trim().is_empty() {
            "!request".to_string()
        } else {
            self.twitch.request_command.trim().to_string()
        };
        self.apple_music.storefront = if self.apple_music.storefront.trim().is_empty() {
            "us".to_string()
        } else {
            self.apple_music.storefront.trim().to_lowercase()
        };
        self.automation.dispatch_hotkey = if self.automation.dispatch_hotkey.trim().is_empty() {
            "F8".to_string()
        } else {
            self.automation.dispatch_hotkey.trim().to_string()
        };
    }
}

impl CommandResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub fn normalize_twitch_oauth_token(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.to_ascii_lowercase().starts_with("oauth:") {
        trimmed.to_string()
    } else {
        format!("oauth:{trimmed}")
    }
}

pub fn compact_log_message(value: &str) -> String {
    const MAX_LOG_MESSAGE_CHARS: usize = 640;

    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length <= MAX_LOG_MESSAGE_CHARS {
        return trimmed.to_string();
    }

    let prefix = trimmed
        .chars()
        .take(MAX_LOG_MESSAGE_CHARS)
        .collect::<String>()
        .trim_end()
        .to_string();
    format!("{prefix}... [truncated]")
}

#[cfg(test)]
mod tests {
    use super::{AutomationControlMode, AutomationHandoffMode, PersistedState};

    #[test]
    fn persisted_state_defaults_missing_handoff_mode() {
        let state: PersistedState = serde_json::from_str(
            r#"{
                "settings": {
                    "twitch": {
                        "channel": "kerdylives",
                        "botUsername": "kerdyknives",
                        "oauthToken": "oauth:test",
                        "requestCommand": "!sr",
                        "autoConnect": true
                    },
                    "requestLimits": {
                        "maxQueueSize": 25,
                        "maxPerUser": 2,
                        "cooldownSeconds": 120,
                        "allowDuplicates": false,
                        "allowLinks": true,
                        "modsBypassLimits": true,
                        "maxTrackMinutes": 10
                    },
                    "appleMusic": {
                        "storefront": "us"
                    },
                    "automation": {
                        "adapter": "ui-automation",
                        "experimentalAutomationEnabled": true
                    }
                },
                "queue": [],
                "logs": []
            }"#,
        )
        .expect("legacy-compatible state should deserialize");

        assert!(state.settings.automation.experimental_automation_enabled);
        assert_eq!(
            state.settings.automation.control_mode,
            AutomationControlMode::StreamerSafe
        );
        assert_eq!(
            state.settings.automation.handoff_mode,
            AutomationHandoffMode::PlayNext
        );
        assert_eq!(state.settings.automation.dispatch_hotkey, "F8");
        assert!(!state.settings.automation.auto_arm_enabled);
    }
}
