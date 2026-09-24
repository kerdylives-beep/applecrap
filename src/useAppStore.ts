import { useCallback, useEffect, useRef, useState } from 'react'
import {
  approveRequest,
  beginTwitchSignIn,
  bindAppEvents,
  bootstrapApp,
  cancelTwitchSignIn,
  checkForUpdates as checkForUpdatesCommand,
  clearQueue,
  connectBot,
  dismissAlert as dismissAlertCommand,
  dispatchNextRequest,
  disconnectBot,
  enqueueManualRequest,
  exportDiagnostics,
  importLegacyState,
  installUpdate as installUpdateCommand,
  openOverlayPreview as openOverlayPreviewCommand,
  openTwitchSignInPage,
  playerControl as playerControlCommand,
  removeRequest,
  reportProblem as reportProblemCommand,
  revealDataFolder,
  saveSettings,
  searchAppleMusic,
  sendRequestToManualReview,
  showPlayer as showPlayerCommand,
  signOutTwitch,
  type PlayerOp,
} from './tauri'
import type {
  AppSettings,
  AppState,
  AuthSlot,
  CommandResult,
  LogEntry,
  QueueItem,
  SearchResult,
  TrackMatch,
  ViewKey,
} from './types'
import { buildDebugSummary, buildFeedbackMailto } from './utils'

const MAX_VISIBLE_LOGS = 80
// Settings save this long after the last edit.
const AUTOSAVE_DELAY_MS = 700

/**
 * Keeps the previous `logs` array when a new snapshot carries the same log
 * lines, so memoized log views skip re-rendering on unrelated state changes.
 */
function withStableLogs(current: AppState | null, next: AppState): AppState {
  if (
    current &&
    current.logs.length === next.logs.length &&
    current.logs[0]?.id === next.logs[0]?.id
  ) {
    return { ...next, logs: current.logs }
  }
  return next
}

const defaultSettings: AppSettings = {
  twitch: {
    channel: '',
    botUsername: '',
    oauthToken: '',
    requestCommand: '!request',
    autoConnect: false,
  },
  requestLimits: {
    maxQueueSize: 25,
    maxPerUser: 2,
    cooldownSeconds: 120,
    allowDuplicates: false,
    allowLinks: true,
    modsBypassLimits: true,
    maxTrackMinutes: 10,
  },
  appleMusic: {
    storefront: 'us',
  },
  player: {
    autoQueue: true,
    audioOutputDevice: '',
    mediaKeys: true,
  },
  overlay: {
    enabled: true,
    port: 4747,
    showQueue: true,
    queueCount: 3,
  },
  channelPoints: {
    enabled: false,
    title: 'Request a song',
    cost: 500,
    pointsOnly: false,
  },
}

export type SaveStatus = 'idle' | 'pending' | 'saving' | 'saved' | 'error'

function errorMessage(error: unknown) {
  // Tauri commands reject with the backend's message as a plain string.
  if (typeof error === 'string' && error) {
    return error
  }
  return error instanceof Error ? error.message : 'The action failed.'
}

export function useAppStore() {
  const [state, setState] = useState<AppState | null>(null)
  const [settingsDraft, setSettingsDraft] = useState<AppSettings>(defaultSettings)
  const [view, setView] = useState<ViewKey>('desk')
  const [manualUser, setManualUser] = useState('')
  const [manualQuery, setManualQuery] = useState('')
  const [reviewId, setReviewId] = useState<string | null>(null)
  const [searchQuery, setSearchQuery] = useState('')
  const [searchResults, setSearchResults] = useState<SearchResult | null>(null)
  const [notice, setNotice] = useState('Starting AppleCrap...')
  const [busyAction, setBusyAction] = useState<string | null>(null)
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle')
  const hydratedDraft = useRef(false)
  const refreshPromise = useRef<Promise<AppState> | null>(null)
  // Bumped on every edit; a save only writes the server's copy back into the
  // draft when nothing was typed while it was in flight.
  const draftRevision = useRef(0)
  const savedRevision = useRef(0)

  const syncState = useCallback((nextState: AppState) => {
    setState((current) => withStableLogs(current, nextState))
    if (!hydratedDraft.current) {
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
    } else if (nextState.settings.twitch.channel) {
      // Signing in as the broadcaster fills in an empty channel; carry that
      // into the draft so the next save doesn't blank it again.
      setSettingsDraft((current) =>
        current.twitch.channel.trim()
          ? current
          : { ...current, twitch: { ...current.twitch, channel: nextState.settings.twitch.channel } },
      )
    }
  }, [])

  const appendLog = useCallback((entry: LogEntry) => {
    setState((current) =>
      current
        ? {
            ...current,
            logs: [entry, ...current.logs.filter((existing) => existing.id !== entry.id)].slice(
              0,
              MAX_VISIBLE_LOGS,
            ),
          }
        : current,
    )
  }, [])

  const refreshState = useCallback(
    async (silent = false) => {
      if (refreshPromise.current) {
        return refreshPromise.current
      }

      refreshPromise.current = (async () => {
        const nextState = await bootstrapApp()
        syncState(nextState)
        if (!silent) {
          setNotice(nextState.storage.warning ?? 'Ready.')
        }
        return nextState
      })()

      try {
        return await refreshPromise.current
      } finally {
        refreshPromise.current = null
      }
    },
    [syncState],
  )

  useEffect(() => {
    let cancelled = false
    let unsubscribe: () => void = () => {}
    const refreshSilently = () => {
      void refreshState(true).catch(() => undefined)
    }
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        refreshSilently()
      }
    }

    // No polling timer: the backend pushes `stateChanged` on every mutation.
    // Focus/visibility refreshes stay as a catch-up for missed events.
    window.addEventListener('focus', refreshSilently)
    document.addEventListener('visibilitychange', handleVisibilityChange)

    void refreshState()
      .then(() => bindAppEvents({ onStateChanged: syncState, onLogAppended: appendLog }))
      .then((unlisten) => {
        if (cancelled) {
          unlisten()
        } else {
          unsubscribe = unlisten
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setNotice(errorMessage(error))
        }
      })

    return () => {
      cancelled = true
      unsubscribe()
      window.removeEventListener('focus', refreshSilently)
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [appendLog, refreshState, syncState])

  const queue = state?.queue ?? []
  const featuredRequest = queue[0] ?? null
  const reviewRequest = queue.find((item) => item.id === reviewId) ?? null

  // The request being reviewed went away (removed, or matched elsewhere).
  useEffect(() => {
    if (reviewId && state && !state.queue.some((item) => item.id === reviewId)) {
      setReviewId(null)
      setSearchResults(null)
    }
  }, [reviewId, state])

  const applyResultNotice = (result: CommandResult) => {
    if (result.message) {
      setNotice(result.message)
    }
  }

  const runAction = async <T,>(actionName: string, work: () => Promise<T>) => {
    setBusyAction(actionName)
    try {
      return await work()
    } catch (error) {
      console.error(error)
      setNotice(errorMessage(error))
      return undefined
    } finally {
      setBusyAction(null)
    }
  }

  const adoptState = (nextState: AppState) => {
    setState(nextState)
    if (draftRevision.current === savedRevision.current) {
      setSettingsDraft(nextState.settings)
    }
    hydratedDraft.current = true
  }

  // --- Settings -------------------------------------------------------------

  const updateDraft = <K extends keyof AppSettings>(section: K, patch: Partial<AppSettings[K]>) => {
    draftRevision.current += 1
    setSaveStatus('pending')
    setSettingsDraft((current) => ({
      ...current,
      [section]: {
        ...current[section],
        ...patch,
      },
    }))
  }

  const saveNow = useCallback(async (draft: AppSettings) => {
    const revision = draftRevision.current
    setSaveStatus('saving')
    try {
      const nextState = await saveSettings(draft)
      savedRevision.current = Math.max(savedRevision.current, revision)
      setState((current) => withStableLogs(current, nextState))
      if (draftRevision.current === revision) {
        setSettingsDraft(nextState.settings)
        setSaveStatus('saved')
      }
      return nextState
    } catch (error) {
      setSaveStatus('error')
      setNotice(errorMessage(error))
      return undefined
    }
  }, [])

  // Autosave: a moment after the last edit.
  useEffect(() => {
    if (draftRevision.current === savedRevision.current || saveStatus !== 'pending') {
      return
    }
    const timer = window.setTimeout(() => {
      void saveNow(settingsDraft)
    }, AUTOSAVE_DELAY_MS)
    return () => window.clearTimeout(timer)
  }, [saveNow, saveStatus, settingsDraft])

  const saveSetting = async <K extends keyof AppSettings>(
    section: K,
    patch: Partial<AppSettings[K]>,
    message: string,
  ) => {
    setSettingsDraft((current) => ({ ...current, [section]: { ...current[section], ...patch } }))
    const nextState = await runAction(`save-${section}`, () => saveSettings({ [section]: patch }))
    if (nextState) {
      adoptState(nextState)
      setNotice(message)
    }
  }

  const setAutoQueueEnabled = (enabled: boolean) =>
    saveSetting('player', { autoQueue: enabled }, enabled ? 'Auto-queue is on.' : 'Auto-queue paused.')

  const setAudioOutputDevice = (deviceId: string) =>
    saveSetting(
      'player',
      { audioOutputDevice: deviceId },
      deviceId ? 'Player audio routed to the selected device.' : 'Player audio routed to the system default.',
    )

  const setMediaKeysEnabled = (enabled: boolean) =>
    saveSetting(
      'player',
      { mediaKeys: enabled },
      enabled ? 'Media keys control AppleCrap while a track is loaded.' : 'Media keys left to other apps.',
    )

  // --- Twitch ---------------------------------------------------------------

  const startBot = async () => {
    // Pending edits (a new channel name, say) must land before connecting.
    if (draftRevision.current !== savedRevision.current) {
      const saved = await saveNow(settingsDraft)
      if (!saved) {
        return
      }
    }
    const nextState = await runAction('connect-bot', connectBot)
    if (nextState) {
      adoptState(nextState)
      setNotice(nextState.botStatus.detail || 'Connecting to Twitch chat.')
    }
  }

  const stopBot = async () => {
    const nextState = await runAction('disconnect-bot', disconnectBot)
    if (nextState) {
      adoptState(nextState)
      setNotice('Chat disconnected.')
    }
  }

  const startTwitchSignIn = async (slot: AuthSlot) => {
    const nextState = await runAction('twitch-sign-in', () => beginTwitchSignIn(slot))
    if (nextState) {
      adoptState(nextState)
      setNotice('Enter the code on Twitch to finish signing in.')
      // Save a click: open the activation page straight away.
      const opened = await openTwitchSignInPage().catch(() => null)
      if (opened && !opened.ok) {
        setNotice(opened.message)
      }
    }
  }

  const reopenTwitchSignInPage = async () => {
    const result = await runAction('twitch-open-page', openTwitchSignInPage)
    if (result) {
      applyResultNotice(result)
    }
  }

  const abortTwitchSignIn = async () => {
    const nextState = await runAction('twitch-cancel-sign-in', cancelTwitchSignIn)
    if (nextState) {
      adoptState(nextState)
      setNotice('Sign-in cancelled.')
    }
  }

  const signOutOfTwitch = async (slot: AuthSlot) => {
    const nextState = await runAction('twitch-sign-out', () => signOutTwitch(slot))
    if (nextState) {
      adoptState(nextState)
      setNotice('Signed out of Twitch.')
    }
  }

  // --- Queue ----------------------------------------------------------------

  const submitManualRequest = async () => {
    const query = manualQuery.trim()
    if (!query) {
      setNotice('Type a song name first.')
      return
    }

    const result = await runAction('manual-request', () =>
      enqueueManualRequest({
        requestedBy: manualUser.trim().replace(/^@/, '') || 'streamer',
        query,
      }),
    )

    if (result) {
      applyResultNotice(result)
      if (result.ok) {
        setManualQuery('')
        setManualUser('')
        await refreshState(true)
      }
    }
  }

  const removeQueueItem = async (id: string) => {
    const nextState = await runAction('remove-request', () => removeRequest(id))
    if (nextState) {
      adoptState(nextState)
      setNotice('Request removed.')
    }
  }

  // A stable identity for list rows: the handler itself is recreated every
  // render, so rows call through a ref that always points at the latest one.
  const removeQueueItemRef = useRef(removeQueueItem)
  removeQueueItemRef.current = removeQueueItem
  const removeQueueItemById = useCallback((id: string) => {
    void removeQueueItemRef.current(id)
  }, [])

  const wipeQueue = async () => {
    const nextState = await runAction('clear-queue', clearQueue)
    if (nextState) {
      adoptState(nextState)
      setNotice('Queue cleared.')
    }
  }

  const searchFor = async (query: string) => {
    const trimmed = query.trim()
    if (!trimmed) {
      setSearchResults(null)
      return
    }
    const result = await runAction('search-apple-music', () => searchAppleMusic(trimmed))
    if (result) {
      setSearchResults(result)
      setNotice(
        result.matches.length
          ? `Found ${result.matches.length} match(es) on Apple Music.`
          : 'Apple Music found nothing for that. Try different words.',
      )
    }
  }

  const startReview = (item: QueueItem) => {
    setReviewId(item.id)
    setSearchQuery(item.query)
    setSearchResults(null)
    void searchFor(item.query)
  }

  // Stable for memoized rows, like removeQueueItemById.
  const queueRef = useRef(queue)
  queueRef.current = queue
  const startReviewRef = useRef(startReview)
  startReviewRef.current = startReview
  const reviewRequestById = useCallback((id: string) => {
    const item = queueRef.current.find((candidate) => candidate.id === id)
    if (item) {
      startReviewRef.current(item)
    }
  }, [])

  const closeReview = () => {
    setReviewId(null)
    setSearchResults(null)
  }

  const approveReviewedRequest = async (track: TrackMatch) => {
    const nextState = await runAction('approve-request', () =>
      approveRequest({ requestId: reviewId, track }),
    )
    if (nextState) {
      adoptState(nextState)
      closeReview()
      setNotice(`Matched to "${track.title}".`)
    }
  }

  const dispatchFeaturedRequest = async () => {
    const nextState = await runAction('dispatch-next', dispatchNextRequest)
    if (nextState) {
      adoptState(nextState)
      setNotice('Sent the next request to the player.')
    }
  }

  const sendToReview = async (id: string) => {
    const nextState = await runAction('manual-review', () => sendRequestToManualReview(id))
    if (nextState) {
      adoptState(nextState)
      setNotice('Request moved to review.')
    }
  }

  // --- Player ---------------------------------------------------------------

  const playerControl = async (op: PlayerOp) => {
    const result = await runAction(`player-${op}`, () => playerControlCommand(op))
    if (result && !result.ok) {
      setNotice(result.message)
    }
  }

  const showPlayer = () => {
    void showPlayerCommand()
  }

  // --- Help, alerts, updates --------------------------------------------------

  const dismissAlert = async (id: string) => {
    const nextState = await runAction('dismiss-alert', () => dismissAlertCommand(id))
    if (nextState) {
      adoptState(nextState)
    }
  }

  const reportProblem = async (alertId?: string) => {
    if (!state) {
      return
    }
    const result = await runAction('report-problem', reportProblemCommand)
    if (!result) {
      return
    }
    if (!result.ok) {
      setNotice(result.message)
      return
    }
    try {
      window.open(buildFeedbackMailto(state, result.message), '_blank')
      setNotice('Report saved and shown in Explorer. Attach it to the email that just opened.')
    } catch (error) {
      console.error(error)
      setNotice(`Report saved to ${result.message}. Email it to kerdylives@gmail.com.`)
    }
    if (alertId) {
      await dismissAlert(alertId)
    }
  }

  const copyDebugSummary = async () => {
    if (!state) {
      return
    }
    try {
      await navigator.clipboard.writeText(buildDebugSummary(state))
      setNotice('Summary copied.')
    } catch {
      setNotice("Couldn't reach the clipboard.")
    }
  }

  const exportLogsAndState = async () => {
    const result = await runAction('export-diagnostics', exportDiagnostics)
    if (result) {
      applyResultNotice(result)
    }
  }

  const openDataFolder = async () => {
    const result = await runAction('reveal-data-folder', revealDataFolder)
    if (result) {
      applyResultNotice(result)
    }
  }

  const importLegacy = async () => {
    const result = await runAction('import-legacy-state', importLegacyState)
    if (result) {
      applyResultNotice(result)
      hydratedDraft.current = false
      const nextState = await refreshState(true)
      adoptState(nextState)
    }
  }

  const checkForUpdates = async () => {
    const nextState = await runAction('check-updates', checkForUpdatesCommand)
    if (nextState) {
      adoptState(nextState)
      setNotice(nextState.update ? `Version ${nextState.update.version} is available.` : "You're on the latest version.")
    }
  }

  const installUpdate = async () => {
    const result = await runAction('install-update', installUpdateCommand)
    if (result) {
      applyResultNotice(result)
    }
  }

  const openOverlayPreview = async () => {
    const result = await runAction('open-overlay-preview', openOverlayPreviewCommand)
    if (result) {
      applyResultNotice(result)
    }
  }

  return {
    state,
    settingsDraft,
    updateDraft,
    saveStatus,
    view,
    setView,
    featuredRequest,
    manualUser,
    setManualUser,
    manualQuery,
    setManualQuery,
    reviewRequest,
    searchQuery,
    setSearchQuery,
    searchResults,
    searchFor,
    startReview,
    reviewRequestById,
    closeReview,
    approveReviewedRequest,
    sendToReview,
    notice,
    setNotice,
    busyAction,
    setAutoQueueEnabled,
    setAudioOutputDevice,
    setMediaKeysEnabled,
    openOverlayPreview,
    startBot,
    stopBot,
    startTwitchSignIn,
    reopenTwitchSignInPage,
    abortTwitchSignIn,
    signOutOfTwitch,
    submitManualRequest,
    removeQueueItemById,
    wipeQueue,
    dispatchFeaturedRequest,
    playerControl,
    showPlayer,
    dismissAlert,
    reportProblem,
    copyDebugSummary,
    exportLogsAndState,
    openDataFolder,
    importLegacy,
    checkForUpdates,
    installUpdate,
  }
}

export type AppStore = ReturnType<typeof useAppStore>
