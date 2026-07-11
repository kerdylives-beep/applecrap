import { useCallback, useEffect, useRef, useState } from 'react'
import {
  approveRequest,
  bindAppEvents,
  bootstrapApp,
  clearQueue,
  connectBot,
  dispatchNextRequest,
  disconnectBot,
  enqueueManualRequest,
  exportDiagnostics,
  importLegacyState,
  installUpdate as installUpdateCommand,
  removeRequest,
  revealDataFolder,
  saveSettings,
  searchAppleMusic,
  sendRequestToManualReview,
} from './tauri'
import type { AppSettings, AppState, CommandResult, PanelKey, SearchResult, TrackMatch } from './types'
import { buildDebugSummary, buildFeedbackMailto } from './utils'

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
  },
}

export function useAppStore() {
  const [state, setState] = useState<AppState | null>(null)
  const [settingsDraft, setSettingsDraft] = useState<AppSettings>(defaultSettings)
  const [activePanel, setActivePanel] = useState<PanelKey>('dashboard')
  const [selectedRequestId, setSelectedRequestId] = useState<string | null>(null)
  const [manualUser, setManualUser] = useState('streamer')
  const [manualQuery, setManualQuery] = useState('')
  const [searchQuery, setSearchQuery] = useState('')
  const [searchResults, setSearchResults] = useState<SearchResult | null>(null)
  const [notice, setNotice] = useState('Booting AppleCrap Alpha...')
  const [busyAction, setBusyAction] = useState<string | null>(null)
  const hydratedDraft = useRef(false)
  const refreshPromise = useRef<Promise<AppState> | null>(null)

  const syncState = useCallback((nextState: AppState) => {
    setState(nextState)
    if (!hydratedDraft.current) {
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
    }
  }, [])

  const refreshState = useCallback(async (silent = false) => {
    if (refreshPromise.current) {
      return refreshPromise.current
    }

    refreshPromise.current = (async () => {
      const nextState = await bootstrapApp()
      syncState(nextState)
      if (!silent) {
        setNotice(nextState.storage.warning ?? 'AppleCrap Alpha is ready.')
      }
      return nextState
    })()

    try {
      return await refreshPromise.current
    } finally {
      refreshPromise.current = null
    }
  }, [syncState])

  useEffect(() => {
    let cancelled = false
    let unsubscribe: () => void = () => {}
    let pollTimer: number | null = null
    const loadState = async (silent = false) => {
      const nextState = await refreshState(silent)
      if (cancelled) {
        return
      }

      return nextState
    }
    const refreshSilently = () => {
      void loadState(true).catch(() => undefined)
    }
    const handleWindowFocus = () => refreshSilently()
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        refreshSilently()
      }
    }

    pollTimer = window.setInterval(() => {
      refreshSilently()
    }, 1500)
    window.addEventListener('focus', handleWindowFocus)
    document.addEventListener('visibilitychange', handleVisibilityChange)

    void loadState()
      .then(() => bindAppEvents(syncState))
      .then((unlisten) => {
        unsubscribe = unlisten
      })
      .catch((error) => {
        if (!cancelled) {
          setNotice(error instanceof Error ? error.message : 'Failed to bootstrap the app.')
        }
      })

    return () => {
      cancelled = true
      unsubscribe()
      if (pollTimer !== null) {
        window.clearInterval(pollTimer)
      }
      window.removeEventListener('focus', handleWindowFocus)
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [refreshState, syncState])

  const queue = state?.queue ?? []
  const featuredRequest = queue[0] ?? null
  const selectedRequest =
    queue.find((item) => item.id === selectedRequestId) ??
    featuredRequest ??
    null

  useEffect(() => {
    if (!selectedRequestId && featuredRequest) {
      setSelectedRequestId(featuredRequest.id)
    }
  }, [featuredRequest, selectedRequestId])

  const applyResultNotice = (result: CommandResult) => {
    setNotice(result.message)
  }

  const runAction = async <T,>(
    actionName: string,
    work: () => Promise<T>,
    onSuccess?: (value: T) => void,
  ) => {
    setBusyAction(actionName)
    try {
      const value = await work()
      onSuccess?.(value)
      return value
    } catch (error) {
      console.error(error)
      setNotice(error instanceof Error ? error.message : 'The action failed.')
      return undefined
    } finally {
      setBusyAction(null)
    }
  }

  const updateDraft = <K extends keyof AppSettings>(section: K, patch: Partial<AppSettings[K]>) => {
    setSettingsDraft((current) => ({
      ...current,
      [section]: {
        ...current[section],
        ...patch,
      },
    }))
  }

  const saveDraftSettings = async () => {
    const nextState = await runAction('save-settings', () => saveSettings(settingsDraft))
    if (nextState) {
      setState(nextState)
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
      setNotice('Settings saved.')
    }
  }

  const setAutoQueueEnabled = async (enabled: boolean) => {
    setSettingsDraft((current) => ({
      ...current,
      player: {
        ...current.player,
        autoQueue: enabled,
      },
    }))

    const nextState = await runAction('toggle-auto-queue', () =>
      saveSettings({
        player: {
          autoQueue: enabled,
        },
      }),
    )

    if (nextState) {
      setState(nextState)
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
      setNotice(enabled ? 'Auto-queue enabled.' : 'Auto-queue paused.')
    }
  }

  const setAudioOutputDevice = async (deviceId: string) => {
    setSettingsDraft((current) => ({
      ...current,
      player: {
        ...current.player,
        audioOutputDevice: deviceId,
      },
    }))

    const nextState = await runAction('set-audio-output', () =>
      saveSettings({
        player: {
          audioOutputDevice: deviceId,
        },
      }),
    )

    if (nextState) {
      setState(nextState)
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
      setNotice(deviceId ? 'Player audio routed to the selected device.' : 'Player audio routed to the system default.')
    }
  }

  const startBot = async () => {
    const savedState = await runAction('save-settings', () => saveSettings(settingsDraft))
    if (!savedState) {
      return
    }

    setState(savedState)
    setSettingsDraft(savedState.settings)
    hydratedDraft.current = true

    const nextState = await runAction('connect-bot', connectBot)
    if (nextState) {
      setState(nextState)
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
      setNotice(nextState.botStatus.detail || 'Bot connected.')
    }
  }

  const stopBot = async () => {
    const nextState = await runAction('disconnect-bot', disconnectBot)
    if (nextState) {
      setState(nextState)
      setNotice('Bot disconnected.')
    }
  }

  const submitManualRequest = async () => {
    const query = manualQuery.trim()
    if (!query) {
      setNotice('Please include a song title or artist.')
      return
    }

    const result = await runAction('manual-request', () =>
      enqueueManualRequest({
        requestedBy: manualUser.trim() || 'streamer',
        query,
      }),
    )

    if (result) {
      applyResultNotice(result)
      if (result.ok) {
        setManualQuery('')
        await refreshState(true)
      }
    }
  }

  const removeQueueItem = async (id: string) => {
    const nextState = await runAction('remove-request', () => removeRequest(id))
    if (nextState) {
      setState(nextState)
      setNotice('Request removed.')
    }
  }

  const wipeQueue = async () => {
    const nextState = await runAction('clear-queue', clearQueue)
    if (nextState) {
      setState(nextState)
      setNotice('Queue cleared.')
    }
  }

  const runSearch = async () => {
    const query = searchQuery.trim()
    if (!query) {
      setSearchResults(null)
      return
    }

    const result = await runAction('search-apple-music', () => searchAppleMusic(query))
    if (result) {
      setSearchResults(result)
      setNotice(`Found ${result.matches.length} Apple Music match(es).`)
    }
  }

  const copyDebugSummary = async () => {
    if (!state) {
      return
    }

    const summary = buildDebugSummary(state)
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(summary)
      setNotice('Debug summary copied.')
      return
    }

    setNotice('Clipboard access is unavailable in this webview.')
  }

  const openFeedbackDraft = async () => {
    if (!state) {
      return
    }

    try {
      const mailto = buildFeedbackMailto(state)
      window.open(mailto, '_blank')
      setNotice('Opened a feedback draft in your default mail app.')
    } catch (error) {
      console.error(error)
      setNotice('Could not open a feedback draft on this machine.')
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
      setState(nextState)
      setSettingsDraft(nextState.settings)
      hydratedDraft.current = true
    }
  }

  const installUpdate = async () => {
    const result = await runAction('install-update', installUpdateCommand)
    if (result) {
      applyResultNotice(result)
    }
  }

  const dispatchFeaturedRequest = async () => {
    const nextState = await runAction('dispatch-next', dispatchNextRequest)
    if (nextState) {
      setState(nextState)
      setNotice('Sent the front request to Apple Music.')
    }
  }

  const approveSelectedRequest = async (track?: TrackMatch | null) => {
    const nextState = await runAction('approve-request', () =>
      approveRequest({
        requestId: selectedRequest?.id ?? null,
        track: track ?? null,
      }),
    )
    if (nextState) {
      setState(nextState)
      setNotice('Request is ready to dispatch.')
    }
  }

  const moveSelectedRequestToManualReview = async () => {
    const id = selectedRequest?.id
    if (!id) {
      setNotice('No request is selected.')
      return
    }

    const nextState = await runAction('manual-review', () => sendRequestToManualReview(id))
    if (nextState) {
      setState(nextState)
      setNotice('Request moved to manual review.')
    }
  }

  return {
    state,
    settingsDraft,
    updateDraft,
    activePanel,
    setActivePanel,
    selectedRequest,
    selectedRequestId,
    setSelectedRequestId,
    featuredRequest,
    manualUser,
    setManualUser,
    manualQuery,
    setManualQuery,
    searchQuery,
    setSearchQuery,
    searchResults,
    notice,
    busyAction,
    saveDraftSettings,
    setAutoQueueEnabled,
    setAudioOutputDevice,
    startBot,
    stopBot,
    submitManualRequest,
    removeQueueItem,
    wipeQueue,
    runSearch,
    approveSelectedRequest,
    moveSelectedRequestToManualReview,
    dispatchFeaturedRequest,
    copyDebugSummary,
    openFeedbackDraft,
    exportLogsAndState,
    openDataFolder,
    importLegacy,
    installUpdate,
  }
}
