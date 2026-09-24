// A stand-in for the Tauri backend so the UI can be worked on (and
// screenshotted) in a plain browser with `npm run dev`. Only loaded in
// development and only outside the app; it never ships.
//
// Pick a scene with ?demo=playing (default), empty, firstrun or alerts.

import type { AppSettings, AppState, LogEntry, QueueItem, TrackMatch } from '../types'

type Handler = (payload: unknown) => void

const scene = new URLSearchParams(window.location.search).get('demo') ?? 'playing'
const now = Date.now()
const ago = (minutes: number) => new Date(now - minutes * 60_000).toISOString()

function track(id: string, title: string, artistName: string, albumName: string, durationMs: number): TrackMatch {
  return { id, title, artistName, albumName, durationMs, url: `https://music.apple.com/us/song/${id}`, artworkUrl: null }
}

function request(
  id: string,
  requestedBy: string,
  source: string,
  query: string,
  found: TrackMatch | null,
  minutesAgo: number,
): QueueItem {
  return {
    id,
    requestedBy,
    query,
    submittedAt: ago(minutesAgo),
    source,
    resolution: found ? 'matched' : 'manual-review',
    track: found,
    handoffState: found ? 'pending-match' : 'manual-review',
    resolvedTrackUrl: found?.url ?? null,
    matchConfidence: found ? 0.94 : null,
    requiresManualReview: !found,
  }
}

const settings: AppSettings = {
  twitch: { channel: 'yourchannel', botUsername: '', oauthToken: '', requestCommand: '!sr', autoConnect: true },
  requestLimits: {
    maxQueueSize: 25,
    maxPerUser: 2,
    cooldownSeconds: 120,
    allowDuplicates: false,
    allowLinks: true,
    modsBypassLimits: true,
    maxTrackMinutes: 10,
  },
  appleMusic: { storefront: 'us' },
  player: { autoQueue: true, audioOutputDevice: '', mediaKeys: true },
  overlay: { enabled: true, port: 4747, showQueue: true, queueCount: 3 },
  channelPoints: { enabled: true, title: 'Request a song', cost: 500, pointsOnly: false },
}

let logId = 0
const log = (minutesAgo: number, message: string, level: LogEntry['level'] = 'info'): LogEntry => ({
  id: `log-${++logId}`,
  level,
  message,
  timestamp: ago(minutesAgo),
})

const nowPlaying = track('1', 'Human Nature', 'Michael Jackson', 'Thriller', 246_000)

const state: AppState = {
  settings,
  queue: [
    request('q1', 'lofi_lena', 'twitch', 'redbone childish gambino', track('2', 'Redbone', 'Childish Gambino', '"Awaken, My Love!"', 327_000), 6),
    request('q2', 'kodakgrain', 'channel-points', 'dreams fleetwood mac', track('3', 'Dreams', 'Fleetwood Mac', 'Rumours', 257_000), 4),
    request('q3', 'basslinebeth', 'twitch', 'that song from the car ad', null, 3),
    request('q4', 'mixtape_max', 'twitch', 'september earth wind and fire', track('4', 'September', 'Earth, Wind & Fire', 'The Best of Earth, Wind & Fire, Vol. 1', 215_000), 1),
  ],
  readyRequest: null,
  logs: [
    log(1, 'Queued "September" for mixtape_max.'),
    log(2, 'Channel Points request from @snackwave: never gonna give you up (The queue already has that song. Your points were refunded.)', 'warn'),
    log(3, 'Twitch request from @basslinebeth: that song from the car ad (Saved "that song from the car ad" for manual Apple Music review.)'),
    log(4, 'Channel Points request from @kodakgrain: dreams fleetwood mac (Queued Dreams by Fleetwood Mac.)'),
    log(6, 'Queued "Redbone" for lofi_lena.'),
    log(9, 'Now playing matched "Human Nature" requested by @vinyl_vibes.'),
    log(12, 'Twitch bot connected to #yourchannel.'),
  ],
  botStatus: {
    connected: true,
    state: 'connected',
    status: 'Connected',
    detail: 'Listening in #yourchannel.',
    channel: 'yourchannel',
    lastEventAt: ago(1),
  },
  probe: {
    source: 'apple-music-web',
    appId: 'Apple Music (embedded player)',
    status: 'Playing',
    title: nowPlaying.title,
    artist: nowPlaying.artistName,
    album: nowPlaying.albumName,
    matchedQueueId: null,
    matched: true,
    confidence: 1,
    explanation: 'Playing the request from @vinyl_vibes.',
    lastError: null,
    sessions: [],
    updatedAt: new Date(now).toISOString(),
    outputDevices: [{ id: 'default', label: 'Speakers' }],
    currentOutput: '',
    bitrate: 256,
    artworkUrl: null,
    durationMs: nowPlaying.durationMs,
    positionMs: 94_000,
  },
  diagnostics: { lastExportPath: null, exportCount: 0, lastSummary: '' },
  legacyImport: { available: false, imported: false, sourcePath: null, message: '' },
  storage: { mode: 'portable', dataDir: 'C:\\AppleCrap\\data', warning: null },
  stats: { totalRequests: 4, unresolvedRequests: 1, matchedRequests: 3, connectedSince: ago(12) },
  update: null,
  auth: { available: true, broadcaster: { login: 'yourchannel', canManageRewards: true }, bot: null, pending: null, error: null },
  channelPoints: { phase: 'live', detail: 'Taking requests through "Request a song" (500 points).' },
  alerts: [],
  nowPlayingRequest: { requestedBy: 'vinyl_vibes', source: 'channel-points' },
}

if (scene === 'empty') {
  state.queue = []
  Object.assign(state.probe, { title: '', artist: '', album: '', status: 'Stopped', positionMs: null, durationMs: null })
  state.nowPlayingRequest = null
}
if (scene === 'firstrun') {
  state.auth.broadcaster = null
  state.queue = []
  state.botStatus = { ...state.botStatus, connected: false, state: 'disconnected', status: 'Offline', detail: 'Not connected yet.', channel: '' }
  state.settings.twitch.channel = ''
  Object.assign(state.probe, { status: 'SignInRequired', title: '' })
}
if (scene === 'alerts') {
  state.update = { version: 'v0.5.1-beta.1', releaseUrl: '', assetUrl: '' }
  state.alerts.push({
    id: 'apple-search',
    tone: 'error',
    title: "Apple Music search isn't working",
    detail: 'Requests will wait for manual review until it is fixed. Apple may have changed their site, which needs an AppleCrap update.',
    action: { kind: 'reportProblem' },
  })
}

// Real album art, looked up the same way the app does.
async function fillArtwork() {
  const lookups: Array<[TrackMatch | null, string]> = [
    [nowPlaying, 'Human Nature Michael Jackson'],
    ...state.queue.map((item) => [item.track, `${item.track?.title} ${item.track?.artistName}`] as [TrackMatch | null, string]),
  ]
  await Promise.all(
    lookups.map(async ([target, term]) => {
      if (!target) {
        return
      }
      try {
        const response = await fetch(`https://itunes.apple.com/search?entity=song&limit=1&term=${encodeURIComponent(term)}`)
        const body = await response.json()
        const art: string | undefined = body.results?.[0]?.artworkUrl100
        if (art) {
          target.artworkUrl = art.replace('100x100bb', '600x600bb')
        }
      } catch {
        // Offline: the initials placeholder stands in.
      }
    }),
  )
  state.probe.artworkUrl = nowPlaying.artworkUrl
}

const listeners = new Map<string, Map<number, Handler>>()
let callbackId = 0

function emit(event: string, payload: unknown) {
  for (const handler of listeners.get(event)?.values() ?? []) {
    handler({ event, id: 0, payload })
  }
}

function snapshot() {
  return structuredClone(state)
}

const ok = (message = '') => ({ ok: true, message })

async function invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
  switch (command) {
    case 'plugin:event|listen': {
      const event = args.event as string
      const handler = (window as unknown as Record<string, Handler>)[`_${args.handler}`]
      if (!listeners.has(event)) {
        listeners.set(event, new Map())
      }
      listeners.get(event)!.set(args.handler as number, handler)
      return args.handler
    }
    case 'plugin:event|unlisten':
      return null
    case 'plugin:app|version':
      return '0.5.0-beta.1'
    case 'save_settings': {
      const payload = args.payload as Partial<AppSettings>
      for (const [section, patch] of Object.entries(payload)) {
        Object.assign(state.settings[section as keyof AppSettings], patch)
      }
      return snapshot()
    }
    case 'player_control':
      if (args.op === 'togglePlayPause') {
        state.probe.status = state.probe.status === 'Playing' ? 'Paused' : 'Playing'
        state.probe.updatedAt = new Date().toISOString()
        emit('stateChanged', snapshot())
      }
      return ok()
    case 'remove_request':
      state.queue = state.queue.filter((item) => item.id !== args.id)
      return snapshot()
    case 'clear_queue':
      state.queue = []
      return snapshot()
    case 'dismiss_alert':
      state.alerts = state.alerts.filter((alert) => alert.id !== args.id)
      return snapshot()
    case 'search_apple_music':
      return { query: args.query, matches: [track('9', 'Glory Box', 'Portishead', 'Dummy', 305_000)] }
    case 'enqueue_manual_request':
    case 'open_twitch_sign_in_page':
    case 'report_problem':
    case 'export_diagnostics':
    case 'reveal_data_folder':
    case 'open_overlay_preview':
    case 'player_show':
      return ok('Demo mode: nothing happened.')
    default:
      return snapshot()
  }
}

export async function install() {
  await Promise.race([fillArtwork(), new Promise((resolve) => setTimeout(resolve, 3000))])
  const globals = window as unknown as Record<string, unknown>
  globals.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined }
  globals.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback: (callback: Handler) => {
      const id = ++callbackId
      ;(window as unknown as Record<string, Handler>)[`_${id}`] = callback
      return id
    },
    unregisterCallback: () => undefined,
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
  }
}
