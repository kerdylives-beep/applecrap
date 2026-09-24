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

/// Playback handoff behaviour. Play Next is the only dispatch mode; matched
/// requests are queued into Apple Music's Playing Next lane and confirmed by
/// the probe loop once they start playing.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerSettings {
    pub auto_queue: bool,
    /// Browser deviceId of the audio output the player routes to.
    /// Empty string = system default.
    pub audio_output_device: String,
    /// Claim the keyboard's media keys while the player has a track loaded.
    pub media_keys: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            auto_queue: true,
            audio_output_device: String::new(),
            media_keys: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioOutputDevice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub twitch: TwitchSettings,
    pub request_limits: RequestLimits,
    pub apple_music: AppleMusicSettings,
    #[serde(alias = "automation")]
    pub player: PlayerSettings,
    #[serde(default)]
    pub overlay: OverlaySettings,
    #[serde(default)]
    pub channel_points: ChannelPointsSettings,
}

/// Song requests redeemed with Channel Points. The app creates and owns the
/// reward on the broadcaster's channel (Twitch only lets the app that created
/// a reward manage its redemptions).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ChannelPointsSettings {
    pub enabled: bool,
    pub title: String,
    pub cost: u32,
    /// Take requests only through Channel Points; `!request` then points
    /// viewers at the reward (mods can still use it).
    pub points_only: bool,
}

pub const DEFAULT_REWARD_TITLE: &str = "Request a song";
/// Twitch's limit on reward titles.
pub const MAX_REWARD_TITLE_CHARS: usize = 45;

impl Default for ChannelPointsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            title: DEFAULT_REWARD_TITLE.to_string(),
            cost: 500,
            points_only: false,
        }
    }
}

/// The reward this app created, remembered so it is updated rather than
/// duplicated. Tied to the broadcaster account that owns it.
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPointsState {
    pub reward_id: Option<String>,
    pub broadcaster_id: Option<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelPointsPhase {
    #[default]
    Off,
    Starting,
    Live,
    Error,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPointsStatus {
    pub phase: ChannelPointsPhase,
    pub detail: String,
}

/// What the overlay page renders. Built fresh per request; the overlay polls.
#[derive(Clone, Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork_url: Option<String>,
    pub requested_by: Option<String>,
    pub show_queue: bool,
    pub queue: Vec<OverlayQueueItem>,
}

#[derive(Clone, Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverlayQueueItem {
    pub title: String,
    pub artist: String,
    pub requested_by: String,
}

/// The browser-source overlay served on localhost for OBS.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct OverlaySettings {
    pub enabled: bool,
    pub port: u16,
    pub show_queue: bool,
    pub queue_count: u8,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 4747,
            show_queue: true,
            queue_count: 3,
        }
    }
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

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSession {
    pub app_id: String,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
}

impl ProbeSnapshot {
    /// Compares everything a viewer would notice, ignoring `updated_at` (which
    /// ticks every probe cycle). Used to skip redundant state broadcasts while
    /// nothing is actually changing.
    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        self.source == other.source
            && self.app_id == other.app_id
            && self.status == other.status
            && self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.matched == other.matched
            && self.matched_queue_id == other.matched_queue_id
            && (self.confidence - other.confidence).abs() < f32::EPSILON
            && self.explanation == other.explanation
            && self.last_error == other.last_error
            && self.sessions == other.sessions
            && self.output_devices == other.output_devices
            && self.current_output == other.current_output
            && self.bitrate == other.bitrate
            && self.artwork_url == other.artwork_url
            && self.duration_ms == other.duration_ms
    }
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
    #[serde(default)]
    pub output_devices: Vec<AudioOutputDevice>,
    #[serde(default)]
    pub current_output: String,
    #[serde(default)]
    pub artwork_url: Option<String>,
    /// Playback bitrate the web player reports, in kbps. Its ceiling is 256
    /// (AAC); lossless is only available in Apple's native apps.
    #[serde(default)]
    pub bitrate: Option<i64>,
    /// Track length and position when this snapshot was taken (see
    /// `updated_at`). Position is left out of `is_equivalent_to` so a
    /// playing track doesn't rebroadcast state every cycle; the UI advances
    /// it locally from the pair.
    #[serde(default)]
    pub duration_ms: Option<i64>,
    #[serde(default)]
    pub position_ms: Option<i64>,
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
            output_devices: Vec::new(),
            current_output: String::new(),
            artwork_url: None,
            bitrate: None,
            duration_ms: None,
            position_ms: None,
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
    /// Signed-in Twitch accounts. Tokens are encrypted on disk and never
    /// included in `AppState`, so they never reach the UI.
    #[serde(default)]
    pub auth: AuthState,
    #[serde(default)]
    pub channel_points: ChannelPointsState,
}

/// Which signed-in Twitch account something belongs to.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthSlot {
    /// The account that chats (often a dedicated bot account).
    Bot,
    /// The channel owner: needed for Channel Points.
    Broadcaster,
}

impl AuthSlot {
    pub fn label(self) -> &'static str {
        match self {
            AuthSlot::Bot => "bot",
            AuthSlot::Broadcaster => "broadcaster",
        }
    }
}

/// A Twitch account signed in through the device-code flow.
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct TwitchAuth {
    pub login: String,
    pub user_id: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds when the access token expires.
    pub expires_at: i64,
    pub scopes: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AuthState {
    pub bot: Option<TwitchAuth>,
    pub broadcaster: Option<TwitchAuth>,
}

impl AuthState {
    pub fn slot(&self, slot: AuthSlot) -> Option<&TwitchAuth> {
        match slot {
            AuthSlot::Bot => self.bot.as_ref(),
            AuthSlot::Broadcaster => self.broadcaster.as_ref(),
        }
    }

    pub fn slot_mut(&mut self, slot: AuthSlot) -> &mut Option<TwitchAuth> {
        match slot {
            AuthSlot::Bot => &mut self.bot,
            AuthSlot::Broadcaster => &mut self.broadcaster,
        }
    }
}

/// What the UI is told about Twitch sign-in. Deliberately token-free.
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthSummary {
    /// False when this build has no Twitch client ID, so sign-in can't run.
    pub available: bool,
    pub bot: Option<SignedInAccount>,
    pub broadcaster: Option<SignedInAccount>,
    pub pending: Option<PendingSignIn>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SignedInAccount {
    pub login: String,
    /// Whether the token carries the Channel Points scopes.
    pub can_manage_rewards: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingSignIn {
    pub slot: AuthSlot,
    pub user_code: String,
    pub verification_uri: String,
    /// Unix seconds when the code stops working.
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub settings: AppSettings,
    pub queue: Vec<QueueItem>,
    pub ready_request: Option<QueueItem>,
    pub logs: Vec<LogEntry>,
    pub bot_status: BotStatus,
    pub probe: ProbeSnapshot,
    pub diagnostics: DiagnosticsSnapshot,
    pub legacy_import: LegacyImportStatus,
    pub storage: StorageInfo,
    pub stats: AppStats,
    pub update: Option<UpdateInfo>,
    pub auth: AuthSummary,
    pub channel_points: ChannelPointsStatus,
    pub alerts: Vec<Alert>,
    /// Who asked for the song playing now, when it came from the queue.
    pub now_playing_request: Option<NowPlayingRequest>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NowPlayingRequest {
    pub requested_by: String,
    /// "twitch", "channel-points" or "dashboard".
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub release_url: String,
    pub asset_url: String,
    /// Detached signature for the zip. Releases without one are offered as a
    /// manual download only.
    #[serde(default)]
    pub signature_url: Option<String>,
    /// This version was installed here before and didn't start, so it was
    /// rolled back. Offered as a manual download only.
    #[serde(default)]
    pub rolled_back: bool,
}

/// A banner for something the user should know about (a rolled-back
/// update, a crash last session, Apple Music search trouble).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// Stable per kind of problem, so raising it again replaces it.
    pub id: String,
    pub tone: AlertTone,
    pub title: String,
    pub detail: String,
    pub action: Option<AlertAction>,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlertTone {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AlertAction {
    ReportProblem,
    OpenUrl { label: String, url: String },
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
pub struct PlayerSettingsPatch {
    pub auto_queue: Option<bool>,
    pub audio_output_device: Option<String>,
    pub media_keys: Option<bool>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsPayload {
    pub twitch: Option<TwitchSettingsPatch>,
    pub request_limits: Option<RequestLimitsPatch>,
    pub apple_music: Option<AppleMusicSettingsPatch>,
    pub player: Option<PlayerSettingsPatch>,
    pub overlay: Option<OverlaySettingsPatch>,
    pub channel_points: Option<ChannelPointsSettingsPatch>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPointsSettingsPatch {
    pub enabled: Option<bool>,
    pub title: Option<String>,
    pub cost: Option<u32>,
    pub points_only: Option<bool>,
}

#[derive(Clone, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettingsPatch {
    pub enabled: Option<bool>,
    pub port: Option<u16>,
    pub show_queue: Option<bool>,
    pub queue_count: Option<u8>,
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

        if let Some(player) = patch.player {
            if let Some(auto_queue) = player.auto_queue {
                self.player.auto_queue = auto_queue;
            }
            if let Some(audio_output_device) = player.audio_output_device {
                self.player.audio_output_device = audio_output_device.trim().to_string();
            }
            if let Some(media_keys) = player.media_keys {
                self.player.media_keys = media_keys;
            }
        }

        if let Some(overlay) = patch.overlay {
            if let Some(enabled) = overlay.enabled {
                self.overlay.enabled = enabled;
            }
            if let Some(port) = overlay.port {
                self.overlay.port = port;
            }
            if let Some(show_queue) = overlay.show_queue {
                self.overlay.show_queue = show_queue;
            }
            if let Some(queue_count) = overlay.queue_count {
                self.overlay.queue_count = queue_count;
            }
        }

        if let Some(channel_points) = patch.channel_points {
            if let Some(enabled) = channel_points.enabled {
                self.channel_points.enabled = enabled;
            }
            if let Some(title) = channel_points.title {
                self.channel_points.title = title;
            }
            if let Some(cost) = channel_points.cost {
                self.channel_points.cost = cost;
            }
            if let Some(points_only) = channel_points.points_only {
                self.channel_points.points_only = points_only;
            }
        }

        self.normalize();
    }

    pub fn normalize(&mut self) {
        let title = self.channel_points.title.trim();
        self.channel_points.title = if title.is_empty() {
            DEFAULT_REWARD_TITLE.to_string()
        } else {
            title.chars().take(MAX_REWARD_TITLE_CHARS).collect()
        };
        self.channel_points.cost = self.channel_points.cost.clamp(1, 10_000_000);

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
    use super::PersistedState;

    /// Old alpha state.json files persisted a full "automation" object
    /// (adapter, controlMode, handoffMode, dispatchHotkey, autoArmEnabled).
    /// That shape no longer matches PlayerSettings, so the aliased `player`
    /// field should simply fall back to its default (auto_queue: true)
    /// rather than erroring out or trying to translate old values.
    #[test]
    fn persisted_state_defaults_player_settings_from_legacy_automation_blob() {
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
                        "controlMode": "streamer-safe",
                        "experimentalAutomationEnabled": true,
                        "handoffMode": "play-next",
                        "dispatchHotkey": "F8",
                        "autoArmEnabled": false
                    }
                },
                "queue": [],
                "logs": []
            }"#,
        )
        .expect("legacy-compatible state should deserialize");

        assert!(state.settings.player.auto_queue);
    }

    #[test]
    fn persisted_state_reads_new_player_settings_shape() {
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
                    "player": {
                        "autoQueue": false
                    }
                },
                "queue": [],
                "logs": []
            }"#,
        )
        .expect("current-shape state should deserialize");

        assert!(!state.settings.player.auto_queue);
    }
}
