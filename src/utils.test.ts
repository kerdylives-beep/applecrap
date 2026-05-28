import { describe, expect, it } from 'vitest'
import { clampText, formatDuration, queueSubline } from './utils'
import type { QueueItem } from './types'

describe('utils', () => {
  it('formats durations in minutes and seconds', () => {
    expect(formatDuration(245000)).toBe('4:05')
  })

  it('clamps long text with ascii ellipsis', () => {
    expect(clampText('abcdefghijklmnopqrstuvwxyz', 10)).toBe('abcdefg...')
  })

  it('builds a queue subline for matched tracks', () => {
    const item: QueueItem = {
      id: '1',
      requestedBy: 'viewer',
      query: 'human nature',
      submittedAt: '2026-01-01T00:00:00.000Z',
      source: 'dashboard',
      resolution: 'matched',
      handoffState: 'pending-match',
      resolvedTrackUrl: 'https://music.apple.com',
      matchConfidence: 0.92,
      requiresManualReview: false,
      track: {
        id: 'track-1',
        title: 'Human Nature',
        artistName: 'Michael Jackson',
        albumName: 'Thriller',
        durationMs: 245000,
        url: 'https://music.apple.com',
      },
      dispatchedAt: null,
    }

    expect(queueSubline(item)).toBe('Thriller | 4:05 | 92% match')
  })
})
