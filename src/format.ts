import type { LogEntry } from './types'

export function cx(...values: Array<string | false | null | undefined>) {
  return values.filter(Boolean).join(' ')
}

// The first letters of up to two words, for artwork placeholders.
export function initials(text: string) {
  const words = text
    .replace(/[^\p{L}\p{N}\s]/gu, ' ')
    .split(/\s+/)
    .filter(Boolean)
  const letters = words.slice(0, 2).map((word) => word[0])
  return letters.join('').toUpperCase() || '?'
}

export function formatClock(ms: number | null | undefined) {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) {
    return '–:––'
  }
  const total = Math.max(0, Math.floor(ms / 1000))
  const minutes = Math.floor(total / 60)
  return `${minutes}:${String(total % 60).padStart(2, '0')}`
}

export function sourceLabel(source: string) {
  switch (source) {
    case 'channel-points':
      return 'points'
    case 'twitch':
      return 'chat'
    case 'dashboard':
      return 'added here'
    default:
      return source
  }
}

export type ActivityMode = 'requests' | 'problems' | 'all'

// How the backend's request-related log lines begin (see app/queue.rs,
// app/channel_points.rs and the chat commands in twitch_service.rs).
const REQUEST_EVENT =
  /^(Queued|Twitch request from|Channel Points request from|Twitch !(remove|skip) from|Removed the latest request|No Apple Music song match|Now playing matched|Catching up on|Couldn't (complete|refund))/

export function activityEntries(logs: LogEntry[], mode: ActivityMode) {
  switch (mode) {
    case 'requests':
      return logs.filter((entry) => entry.level !== 'debug' && REQUEST_EVENT.test(entry.message))
    case 'problems':
      return logs.filter((entry) => entry.level === 'warn' || entry.level === 'error')
    default:
      return logs
  }
}
