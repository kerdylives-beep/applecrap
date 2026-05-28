import { EventEmitter } from 'node:events'
import fs from 'node:fs'
import path from 'node:path'
import { randomUUID } from 'node:crypto'
import { z } from 'zod'

const settingsSchema = z.object({
  twitch: z.object({
    channel: z.string().default(''),
    botUsername: z.string().default(''),
    oauthToken: z.string().default(''),
    requestCommand: z.string().default('!request'),
    autoConnect: z.boolean().default(false),
  }).default({}),
  requestLimits: z.object({
    maxQueueSize: z.number().int().min(1).max(500).default(25),
    maxPerUser: z.number().int().min(1).max(20).default(2),
    cooldownSeconds: z.number().int().min(0).max(3600).default(120),
    allowDuplicates: z.boolean().default(false),
    allowLinks: z.boolean().default(false),
    modsBypassLimits: z.boolean().default(true),
    maxTrackMinutes: z.number().int().min(1).max(30).default(10),
  }).default({}),
  appleMusic: z.object({
    developerToken: z.string().default(''),
    storefront: z.string().default('us'),
  }).default({}),
})

const persistedStateSchema = z.object({
  settings: settingsSchema,
  queue: z.array(z.any()).default([]),
  logs: z.array(z.any()).default([]),
})

export const defaultSettings = settingsSchema.parse({})

export class StateManager extends EventEmitter {
  constructor(statePath, appleMusicService) {
    super()
    this.statePath = statePath
    this.appleMusicService = appleMusicService
    this.state = this.loadState()
    this.botStatus = { connected: false, status: 'Disconnected', detail: 'Bot is offline.' }
    this.nowPlaying = {
      source: 'idle',
      appId: '',
      status: 'Stopped',
      title: '',
      artist: '',
      album: '',
      matchedQueueId: null,
      matched: false,
      debugSessions: [],
    }
  }

  loadState() {
    if (!fs.existsSync(this.statePath)) {
      return { settings: defaultSettings, queue: [], logs: [] }
    }

    try {
      const raw = fs.readFileSync(this.statePath, 'utf8')
      return persistedStateSchema.parse(JSON.parse(raw))
    } catch {
      return { settings: defaultSettings, queue: [], logs: [] }
    }
  }

  save() {
    fs.mkdirSync(path.dirname(this.statePath), { recursive: true })
    fs.writeFileSync(this.statePath, JSON.stringify(this.state, null, 2))
  }

  snapshot() {
    return {
      settings: this.state.settings,
      queue: this.state.queue,
      logs: this.state.logs.slice(0, 40),
      botStatus: this.botStatus,
      nowPlaying: this.nowPlaying,
      stats: {
        totalRequests: this.state.queue.length,
        unresolvedRequests: this.state.queue.filter((item) => !item.track).length,
      },
    }
  }

  emitState() {
    this.emit('state', this.snapshot())
  }

  addLog(level, message) {
    this.state.logs.unshift({
      id: randomUUID(),
      level,
      message,
      timestamp: new Date().toISOString(),
    })
    this.state.logs = this.state.logs.slice(0, 100)
    this.save()
    this.emitState()
  }

  setBotStatus(status) {
    this.botStatus = status
    this.emitState()
  }

  setNowPlaying(nowPlaying) {
    this.nowPlaying = nowPlaying
    this.emitState()
  }

  updateSettings(partialSettings) {
    const merged = {
      ...this.state.settings,
      ...partialSettings,
      twitch: {
        ...this.state.settings.twitch,
        ...(partialSettings.twitch ?? {}),
      },
      requestLimits: {
        ...this.state.settings.requestLimits,
        ...(partialSettings.requestLimits ?? {}),
      },
      appleMusic: {
        ...this.state.settings.appleMusic,
        ...(partialSettings.appleMusic ?? {}),
      },
    }

    this.state.settings = settingsSchema.parse(merged)
    this.save()
    this.emitState()
    return this.state.settings
  }

  removeRequest(id) {
    this.state.queue = this.state.queue.filter((item) => item.id !== id)
    this.save()
    this.emitState()
  }

  clearQueue() {
    this.state.queue = []
    this.save()
    this.emitState()
  }

  removeLatestRequestByUser(requestedBy) {
    const normalizedName = requestedBy.toLowerCase()
    const item = [...this.state.queue].reverse().find((entry) => entry.requestedBy.toLowerCase() === normalizedName)
    if (!item) {
      return { removed: false, message: 'You do not have any active requests to remove.' }
    }

    this.state.queue = this.state.queue.filter((entry) => entry.id !== item.id)
    this.save()
    this.emitState()
    this.addLog('info', `Removed the latest request for ${requestedBy}.`)
    return { removed: true, message: 'Removed your most recent request.' }
  }

  async createManualRequest(requestedBy, query) {
    return this.processRequest({
      requestedBy,
      query,
      isPrivileged: true,
      source: 'dashboard',
    })
  }

  async processRequest({ requestedBy, query, isPrivileged = false, source = 'twitch' }) {
    const settings = this.state.settings
    const normalizedQuery = query.trim()
    if (!normalizedQuery) {
      return { accepted: false, message: 'Please include a song title or artist.' }
    }

    if (!settings.requestLimits.allowLinks && /^https?:\/\//i.test(normalizedQuery)) {
      return { accepted: false, message: 'Links are disabled. Request by song title instead.' }
    }

    if (this.state.queue.length >= settings.requestLimits.maxQueueSize && !isPrivileged) {
      return { accepted: false, message: 'The request queue is full right now.' }
    }

    const userRequests = this.state.queue.filter((item) => item.requestedBy.toLowerCase() === requestedBy.toLowerCase())
    if (userRequests.length >= settings.requestLimits.maxPerUser && !isPrivileged) {
      return { accepted: false, message: `You already have ${settings.requestLimits.maxPerUser} active request(s).` }
    }

    const latestRequest = userRequests
      .map((item) => Date.parse(item.submittedAt))
      .sort((a, b) => b - a)[0]

    if (
      latestRequest &&
      settings.requestLimits.cooldownSeconds > 0 &&
      Date.now() - latestRequest < settings.requestLimits.cooldownSeconds * 1000 &&
      !isPrivileged
    ) {
      return { accepted: false, message: 'Please wait before requesting another song.' }
    }

    let track = null
    let resolution = 'manual-review'

    try {
      const match = await this.appleMusicService.searchTopTrack(normalizedQuery, settings.appleMusic)
      if (match) {
        if (
          !settings.requestLimits.allowDuplicates &&
          this.state.queue.some((item) => item.track?.id === match.id)
        ) {
          return { accepted: false, message: 'That song is already in the queue.' }
        }

        if (
          match.durationMs &&
          match.durationMs > settings.requestLimits.maxTrackMinutes * 60 * 1000 &&
          !isPrivileged
        ) {
          return { accepted: false, message: `Songs longer than ${settings.requestLimits.maxTrackMinutes} minutes are blocked.` }
        }

        track = match
        resolution = 'matched'
      }
    } catch (error) {
      this.addLog('warn', `Apple Music lookup failed for "${normalizedQuery}": ${error.message}`)
    }

    const request = {
      id: randomUUID(),
      requestedBy,
      query: normalizedQuery,
      submittedAt: new Date().toISOString(),
      source,
      resolution,
      track,
    }

    this.state.queue.push(request)
    this.save()
    this.emitState()

    if (track) {
      this.addLog('info', `Queued "${track.title}" for ${requestedBy}.`)
      return {
        accepted: true,
        message: `Queued ${track.title} by ${track.artistName}.`,
        request,
      }
    }

    this.addLog('info', `Queued manual review request "${normalizedQuery}" for ${requestedBy}.`)
    return {
      accepted: true,
      message: `Saved "${normalizedQuery}" for manual Apple Music review.`,
      request,
    }
  }
}
