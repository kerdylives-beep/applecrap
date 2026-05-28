export type ResolutionStatus = 'matched' | 'manual-review'
export type LogLevel = 'info' | 'warn' | 'error' | 'debug'
export type StorageMode = 'portable' | 'fallback'
export type BotConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error'
export type AutomationAdapterKind = 'deep-link' | 'ui-automation'
export type AutomationHandoffMode = 'play-now' | 'play-next'
export type AutomationControlMode = 'streamer-safe' | 'desktop-automation'
export type QueueHandoffState =
  | 'pending-match'
  | 'ready-to-send'
  | 'sent-to-player'
  | 'confirmed-playing'
  | 'manual-review'
  | 'failed-dispatch'
export type AutomationAction =
  | 'probe_capabilities'
  | 'focus_player'
  | 'open_track'
  | 'attempt_queue_action'
  | 'attempt_play'
  | 'dry_run'
export type PanelKey =
  | 'dashboard'
  | 'bot'
  | 'rules'
  | 'automation'
  | 'now-playing'
  | 'logs'
  | 'about'
  | 'debug'

export type TrackMatch = {
  id: string
  title: string
  artistName: string
  albumName: string
  durationMs: number | null
  url: string
  artworkUrl?: string | null
}

export type QueueItem = {
  id: string
  requestedBy: string
  query: string
  submittedAt: string
  source: string
  resolution: ResolutionStatus
  track: TrackMatch | null
  handoffState: QueueHandoffState
  resolvedTrackUrl: string | null
  matchConfidence: number | null
  requiresManualReview: boolean
  handoffNote?: string | null
  handoffUpdatedAt?: string | null
  dispatchedAt?: string | null
}

export type AppSettings = {
  twitch: {
    channel: string
    botUsername: string
    oauthToken: string
    requestCommand: string
    autoConnect: boolean
  }
  requestLimits: {
    maxQueueSize: number
    maxPerUser: number
    cooldownSeconds: number
    allowDuplicates: boolean
    allowLinks: boolean
    modsBypassLimits: boolean
    maxTrackMinutes: number
  }
  appleMusic: {
    storefront: string
  }
  automation: {
    adapter: AutomationAdapterKind
    controlMode: AutomationControlMode
    experimentalAutomationEnabled: boolean
    handoffMode: AutomationHandoffMode
    dispatchHotkey: string
    autoArmEnabled: boolean
  }
}

export type BotStatus = {
  connected: boolean
  state: BotConnectionState
  status: string
  detail: string
  channel: string
  lastEventAt: string | null
}

export type LogEntry = {
  id: string
  level: LogLevel
  message: string
  timestamp: string
}

export type ProbeSession = {
  appId: string
  status: string
  title: string
  artist: string
  album: string
}

export type ProbeSnapshot = {
  source: string
  appId: string
  status: string
  title: string
  artist: string
  album: string
  matchedQueueId: string | null
  matched: boolean
  confidence: number
  explanation: string
  lastError: string | null
  sessions: ProbeSession[]
  updatedAt: string | null
}

export type AutomationCapabilities = {
  adapter: AutomationAdapterKind
  supportedActions: AutomationAction[]
  canFocusPlayer: boolean
  canOpenTrack: boolean
  canQueueAction: boolean
  canPlay: boolean
}

export type AutomationRunResult = {
  adapter: AutomationAdapterKind
  action: AutomationAction
  ok: boolean
  summary: string
  detail: string
  timestamp: string
}

export type AutomationSnapshot = {
  activeAdapter: AutomationAdapterKind
  experimentalEnabled: boolean
  capabilities: AutomationCapabilities[]
  lastRun: AutomationRunResult | null
}

export type DiagnosticsSnapshot = {
  lastExportPath: string | null
  exportCount: number
  lastSummary: string
}

export type LegacyImportStatus = {
  available: boolean
  imported: boolean
  sourcePath: string | null
  message: string
}

export type StorageInfo = {
  mode: StorageMode
  dataDir: string
  warning: string | null
}

export type AppStats = {
  totalRequests: number
  unresolvedRequests: number
  matchedRequests: number
  connectedSince: string | null
}

export type AppState = {
  settings: AppSettings
  queue: QueueItem[]
  readyRequest: QueueItem | null
  controlMode: AutomationControlMode
  logs: LogEntry[]
  botStatus: BotStatus
  probe: ProbeSnapshot
  automation: AutomationSnapshot
  diagnostics: DiagnosticsSnapshot
  legacyImport: LegacyImportStatus
  storage: StorageInfo
  stats: AppStats
}

export type ManualRequestPayload = {
  requestedBy: string
  query: string
}

export type SaveSettingsPayload = {
  twitch?: Partial<AppSettings['twitch']>
  requestLimits?: Partial<AppSettings['requestLimits']>
  appleMusic?: Partial<AppSettings['appleMusic']>
  automation?: Partial<AppSettings['automation']>
}

export type CommandResult = {
  ok: boolean
  message: string
}

export type SearchResult = {
  query: string
  matches: TrackMatch[]
}

export type ProbeResult = {
  snapshot: ProbeSnapshot
}

export type RunAutomationPayload = {
  adapter: AutomationAdapterKind
  action: AutomationAction
  requestId?: string | null
  dryRun?: boolean
  allowInStreamerSafeMode?: boolean
}

export type OpenTrackPayload = {
  requestId?: string | null
  url?: string | null
  query?: string | null
  allowInStreamerSafeMode?: boolean
}

export type ApproveRequestPayload = {
  requestId?: string | null
  track?: TrackMatch | null
}
