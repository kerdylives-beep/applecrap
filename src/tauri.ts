import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  ApproveRequestPayload,
  AppState,
  CommandResult,
  ManualRequestPayload,
  OpenTrackPayload,
  ProbeResult,
  RunAutomationPayload,
  SaveSettingsPayload,
  SearchResult,
} from './types'

export const appEvents = {
  stateChanged: 'stateChanged',
  logAppended: 'logAppended',
  probeSnapshot: 'probeSnapshot',
  automationSnapshot: 'automationSnapshot',
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

export async function runAutomationStep(payload: RunAutomationPayload) {
  return invoke<CommandResult>('run_automation_step', { payload })
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

export async function setDispatchHotkey(shortcut: string) {
  return invoke<AppState>('set_dispatch_hotkey', { shortcut })
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

export async function bindAppEvents(onStateChanged: (payload: AppState) => void) {
  const unlisteners = await Promise.all([
    listen<AppState>(appEvents.stateChanged, (event) => onStateChanged(event.payload)),
    listen(appEvents.logAppended, () => undefined),
    listen(appEvents.probeSnapshot, () => undefined),
    listen(appEvents.automationSnapshot, () => undefined),
  ])

  return () => {
    for (const unlisten of unlisteners) {
      unlisten()
    }
  }
}
