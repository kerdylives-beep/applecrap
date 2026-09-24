import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ApproveRequestPayload,
  AppState,
  AuthSlot,
  CommandResult,
  LogEntry,
  ManualRequestPayload,
  OpenTrackPayload,
  ProbeResult,
  SaveSettingsPayload,
  SearchResult,
} from './types'

export const appEvents = {
  stateChanged: 'stateChanged',
  logAppended: 'logAppended',
  probeSnapshot: 'probeSnapshot',
} as const

export async function bootstrapApp() {
  return invoke<AppState>('bootstrap_app')
}

export async function saveSettings(payload: SaveSettingsPayload) {
  return invoke<AppState>('save_settings', { payload })
}

export async function connectBot() {
  return invoke<AppState>('connect_bot')
}

export async function disconnectBot() {
  return invoke<AppState>('disconnect_bot')
}

export async function beginTwitchSignIn(slot: AuthSlot) {
  return invoke<AppState>('begin_twitch_sign_in', { slot })
}

export async function cancelTwitchSignIn() {
  return invoke<AppState>('cancel_twitch_sign_in')
}

export async function openTwitchSignInPage() {
  return invoke<CommandResult>('open_twitch_sign_in_page')
}

export async function signOutTwitch(slot: AuthSlot) {
  return invoke<AppState>('sign_out_twitch', { slot })
}

export async function enqueueManualRequest(payload: ManualRequestPayload) {
  return invoke<CommandResult>('enqueue_manual_request', { payload })
}

export async function removeRequest(id: string) {
  return invoke<AppState>('remove_request', { id })
}

export async function clearQueue() {
  return invoke<AppState>('clear_queue')
}

export async function searchAppleMusic(query: string) {
  return invoke<SearchResult>('search_apple_music', { query })
}

export async function openTrack(payload: OpenTrackPayload) {
  return invoke<CommandResult>('open_track', { payload })
}

export async function runProbe() {
  return invoke<ProbeResult>('run_probe')
}

export async function dispatchNextRequest() {
  return invoke<AppState>('dispatch_next_request')
}

export async function approveRequest(payload: ApproveRequestPayload) {
  return invoke<AppState>('approve_request', { payload })
}

export async function sendRequestToManualReview(id: string) {
  return invoke<AppState>('send_request_to_manual_review', { id })
}

export async function exportDiagnostics() {
  return invoke<CommandResult>('export_diagnostics')
}

export async function revealDataFolder() {
  return invoke<CommandResult>('reveal_data_folder')
}

export async function importLegacyState() {
  return invoke<CommandResult>('import_legacy_state')
}

export async function openOverlayPreview() {
  return invoke<CommandResult>('open_overlay_preview')
}

export async function installUpdate() {
  return invoke<CommandResult>('install_update')
}

export async function bindAppEvents(handlers: {
  onStateChanged: (payload: AppState) => void
  // Log lines arrive on their own event so a new line doesn't require the
  // backend to re-send (and the UI to re-render) the whole app state.
  onLogAppended: (entry: LogEntry) => void
}) {
  const unlisteners = await Promise.all([
    listen<AppState>(appEvents.stateChanged, (event) => handlers.onStateChanged(event.payload)),
    listen<LogEntry>(appEvents.logAppended, (event) => handlers.onLogAppended(event.payload)),
    listen(appEvents.probeSnapshot, () => undefined),
  ])

  return () => {
    for (const unlisten of unlisteners) {
      unlisten()
    }
  }
}
