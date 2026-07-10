import type { AppState, LogEntry, ProbeSnapshot, QueueItem } from './types'

export type PlayerConnectionState = 'connected' | 'loading' | 'sign-in-required' | 'disconnected'

/**
 * Derives the embedded player's connection state from the probe snapshot.
 * The player bridge tags every report with source "apple-music-web" once
 * it has heard from the player window at least once; before that (or if
 * the window has gone away) the probe stays at its idle default.
 */
export function playerConnectionState(probe: ProbeSnapshot): PlayerConnectionState {
  if (probe.source !== 'apple-music-web') {
    return 'disconnected'
  }

  switch (probe.status) {
    case 'SignInRequired':
      return 'sign-in-required'
    case 'Loading':
      return 'loading'
    case 'Disconnected':
      return 'disconnected'
    default:
      return 'connected'
  }
}

export function playerStatusLabel(state: PlayerConnectionState) {
  switch (state) {
    case 'connected':
      return 'Connected'
    case 'loading':
      return 'Loading'
    case 'sign-in-required':
      return 'Sign-in required'
    case 'disconnected':
      return 'Disconnected'
  }
}

export function playerNowPlayingLine(probe: ProbeSnapshot) {
  if (probe.title) {
    return probe.artist ? `${probe.title} — ${probe.artist}` : probe.title
  }

  if (probe.status && probe.status !== 'Unknown') {
    return probe.status
  }

  return 'Nothing playing yet.'
}

export function formatDuration(durationMs: number | null) {
  if (!durationMs) {
    return 'Unknown length'
  }

  const totalSeconds = Math.max(0, Math.round(durationMs / 1000))
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}:${seconds.toString().padStart(2, '0')}`
}

export function formatTimestamp(value: string | null) {
  if (!value) {
    return 'Never'
  }

  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(new Date(value))
}

export function queueHeadline(item: QueueItem) {
  return item.track ? `${item.track.title} - ${item.track.artistName}` : item.query
}

export function queueSubline(item: QueueItem) {
  if (item.handoffState === 'failed-dispatch' && item.handoffNote) {
    return item.handoffNote
  }

  if (
    (item.handoffState === 'ready-to-send' ||
      item.handoffState === 'sent-to-player' ||
      item.handoffState === 'confirmed-playing') &&
    item.handoffNote
  ) {
    return item.handoffNote
  }

  if (item.requiresManualReview || !item.track) {
    return 'Manual review required in Apple Music'
  }

  const pieces = [
    item.track.albumName,
    formatDuration(item.track.durationMs),
    item.matchConfidence !== null ? `${Math.round(item.matchConfidence * 100)}% match` : null,
  ].filter(Boolean)
  return pieces.join(' | ')
}

export function queueStatusLabel(item: QueueItem) {
  if (item.handoffState === 'manual-review' || item.requiresManualReview || item.resolution === 'manual-review') {
    return 'Review'
  }

  if (item.handoffState === 'failed-dispatch') {
    return 'Needs retry'
  }

  if (item.handoffState === 'confirmed-playing') {
    return 'Playing'
  }

  if (item.handoffState === 'sent-to-player') {
    return 'Dispatched'
  }

  if (item.handoffState === 'ready-to-send') {
    return 'Ready'
  }

  if (item.handoffState === 'pending-match') {
    return 'Pending'
  }

  return 'Queued'
}

export function queueStatusTone(item: QueueItem) {
  if (item.handoffState === 'manual-review' || item.requiresManualReview || item.handoffState === 'failed-dispatch') {
    return 'warn'
  }

  if (item.handoffState === 'ready-to-send' || item.handoffState === 'sent-to-player' || item.handoffState === 'confirmed-playing') {
    return 'good'
  }

  return 'neutral'
}

export function logSummary(logs: LogEntry[]) {
  if (!logs.length) {
    return 'No events recorded yet.'
  }

  return logs
    .slice(0, 20)
    .map((entry) => `[${entry.level.toUpperCase()}] ${formatTimestamp(entry.timestamp)} ${entry.message}`)
    .join('\n')
}

export function buildDebugSummary(state: AppState) {
  const headline = [
    `Storage: ${state.storage.mode} (${state.storage.dataDir})`,
    `Bot: ${state.botStatus.status} (${state.botStatus.channel || 'no channel'})`,
    `Command: ${state.settings.twitch.requestCommand || '!request'}`,
    `Links: ${state.settings.requestLimits.allowLinks ? 'allowed' : 'blocked'}`,
    `Queue: ${state.stats.totalRequests} item(s)`,
    `Probe: ${state.probe.status || 'Unknown'} (${state.probe.source || 'idle'})`,
    `Auto-queue: ${state.settings.player.autoQueue ? 'on' : 'off'}`,
  ].join('\n')

  return [headline, '', logSummary(state.logs)].join('\n')
}

export function buildFeedbackMailto(state: AppState) {
  const subject = 'AppleCrap Alpha Feedback'
  const body = [
    'What happened:',
    '',
    '',
    'What I expected:',
    '',
    '',
    'Optional notes:',
    '',
    '',
    '--- Debug summary ---',
    buildDebugSummary(state),
  ].join('\n')

  return `mailto:kerdylives@gmail.com?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`
}

export function clampText(value: string, max = 120) {
  if (value.length <= max) {
    return value
  }

  return `${value.slice(0, max - 3)}...`
}

export function emptyStateMessage(command: string) {
  const variants = [
    'Queue is clear. Put chat to work.',
    `No songs on deck yet. Remind chat to use ${command}.`,
    'The handoff lane is empty. Time for a request.',
  ]

  const index = new Date().getMinutes() % variants.length
  return variants[index]
}
