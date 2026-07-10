import { useDeferredValue, useMemo, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import faviconUrl from '../img/favicon.ico'
import styles from './App.module.css'
import { useAppStore } from './useAppStore'
import type { LogEntry, LogLevel, PanelKey, QueueItem } from './types'
import {
  clampText,
  emptyStateMessage,
  formatDuration,
  formatTimestamp,
  queueHeadline,
  queueStatusLabel,
  queueStatusTone,
  queueSubline,
} from './utils'

type LogFilter = 'all' | LogLevel

const utilityPanels: Array<{ key: Exclude<PanelKey, 'dashboard'>; label: string }> = [
  { key: 'bot', label: 'Bot' },
  { key: 'rules', label: 'Rules' },
  { key: 'automation', label: 'Automation' },
  { key: 'now-playing', label: 'Now Playing' },
  { key: 'logs', label: 'Logs' },
  { key: 'about', label: 'About' },
  { key: 'debug', label: 'Debug' },
]

function cx(...values: Array<string | false | null | undefined>) {
  return values.filter(Boolean).join(' ')
}

function SectionFrame({
  title,
  eyebrow,
  actions,
  children,
}: {
  title: string
  eyebrow: string
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <section className={styles.sectionFrame}>
      <header className={styles.sectionHeader}>
        <div className={styles.sectionHeading}>
          <p className={styles.eyebrow}>{eyebrow}</p>
          <h2>{title}</h2>
        </div>
        {actions ? <div className={styles.sectionActions}>{actions}</div> : null}
      </header>
      {children}
    </section>
  )
}

function Pill({
  label,
  value,
  tone = 'neutral',
}: {
  label: string
  value: string
  tone?: 'neutral' | 'good' | 'warn'
}) {
  return (
    <div className={cx(styles.pill, tone === 'good' && styles.pillGood, tone === 'warn' && styles.pillWarn)}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
}

function StatusChip({
  label,
  tone = 'neutral',
}: {
  label: string
  tone?: 'neutral' | 'good' | 'warn'
}) {
  return (
    <span
      className={cx(
        styles.statusChip,
        tone === 'good' && styles.statusChipGood,
        tone === 'warn' && styles.statusChipWarn,
      )}
    >
      {label}
    </span>
  )
}

function QueueRow({
  item,
  active,
  openLabel,
  openDisabled,
  onSelect,
  onOpen,
  onRemove,
}: {
  item: QueueItem
  active: boolean
  openLabel: string
  openDisabled: boolean
  onSelect: () => void
  onOpen: () => void
  onRemove: () => void
}) {
  return (
    <button type="button" className={cx(styles.queueRow, active && styles.queueRowActive)} onClick={onSelect}>
      <div className={styles.queuePrimary}>
        <strong>{queueHeadline(item)}</strong>
        <span>{queueSubline(item)}</span>
      </div>
      <div className={styles.queueMeta}>
        <span className={styles.queueRequester}>@{item.requestedBy}</span>
        <StatusChip label={queueStatusLabel(item)} tone={queueStatusTone(item)} />
        <span className={styles.queueTime}>{formatTimestamp(item.submittedAt)}</span>
      </div>
      <div className={styles.queueActions}>
        <button
          type="button"
          className={styles.secondaryButton}
          disabled={openDisabled}
          onClick={(event) => {
            event.stopPropagation()
            onOpen()
          }}
        >
          {openLabel}
        </button>
        <button
          type="button"
          className={styles.ghostButton}
          onClick={(event) => {
            event.stopPropagation()
            onRemove()
          }}
        >
          Remove
        </button>
      </div>
    </button>
  )
}

function logToneClass(level: LogLevel) {
  switch (level) {
    case 'info':
      return styles.log_info
    case 'warn':
      return styles.log_warn
    case 'error':
      return styles.log_error
    case 'debug':
      return styles.log_debug
    default:
      return ''
  }
}

function LogList({ entries }: { entries: LogEntry[] }) {
  if (!entries.length) {
    return <p className={styles.emptyCopy}>No log lines match the current filter.</p>
  }

  return (
    <div className={styles.logList}>
      {entries.map((entry) => (
        <article key={entry.id} className={styles.logEntry}>
          <strong className={cx(styles.logLevel, logToneClass(entry.level))}>{entry.level}</strong>
          <div>
            <p>{entry.message}</p>
            <span>{formatTimestamp(entry.timestamp)}</span>
          </div>
        </article>
      ))}
    </div>
  )
}

function ModalShell({
  title,
  eyebrow,
  onClose,
  actions,
  children,
}: {
  title: string
  eyebrow: string
  onClose: () => void
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <div className={styles.modalBackdrop} onClick={onClose}>
      <section className={styles.modalSheet} onClick={(event) => event.stopPropagation()}>
        <header className={styles.modalHeader}>
          <div className={styles.sectionHeading}>
            <p className={styles.eyebrow}>{eyebrow}</p>
            <h2>{title}</h2>
          </div>
          <div className={styles.modalActions}>
            {actions}
            <button className={styles.ghostButton} onClick={onClose}>
              Close
            </button>
          </div>
        </header>
        <div className={styles.modalBody}>{children}</div>
      </section>
    </div>
  )
}

function App() {
  const store = useAppStore()
  const [logFilter, setLogFilter] = useState<LogFilter>('all')
  const deferredLogFilter = useDeferredValue(logFilter)
  const state = store.state

  const filteredLogs = useMemo(() => {
    if (!state) {
      return []
    }

    return deferredLogFilter === 'all'
      ? state.logs
      : state.logs.filter((entry) => entry.level === deferredLogFilter)
  }, [deferredLogFilter, state])

  if (!state) {
    return (
      <main className={styles.loadingShell}>
        <div className={styles.loadingCard}>
          <p className={styles.eyebrow}>AppleCrap Alpha</p>
          <h1>Preparing the handoff desk</h1>
          <p>{store.notice}</p>
        </div>
      </main>
    )
  }

  const queue = state.queue
  const featuredRequest = store.featuredRequest
  const requestCommand = state.settings.twitch.requestCommand || '!request'
  const controlMode = store.settingsDraft.automation.controlMode
  const experimentalOpenAndPlayEnabled =
    store.settingsDraft.automation.experimentalAutomationEnabled &&
    store.settingsDraft.automation.adapter === 'ui-automation'
  const autoArmEnabled = store.settingsDraft.automation.autoArmEnabled
  const experimentalPlayNextEnabled =
    experimentalOpenAndPlayEnabled && store.settingsDraft.automation.handoffMode === 'play-next'
  const modalPanel = store.activePanel === 'dashboard' ? null : store.activePanel
  const primaryHandoffLabel =
    controlMode === 'streamer-safe'
      ? 'Dispatch'
      : experimentalOpenAndPlayEnabled
        ? experimentalPlayNextEnabled
          ? 'Play next'
          : 'Open + play'
        : 'Open in Apple Music'
  const stagedRequests = queue.filter((item) => item.handoffState === 'sent-to-player').length
  const waitingRequests = queue.filter((item) => item.handoffState === 'pending-match').length

  const getQueueActionLabel = (item: QueueItem) => {
    if (controlMode === 'streamer-safe') {
      if (item.handoffState === 'manual-review' || item.requiresManualReview || !item.track) {
        return 'Review'
      }
      if (item.handoffState === 'ready-to-send') {
        return 'Ready'
      }
      if (item.handoffState === 'sent-to-player') {
        return 'Sent'
      }
      if (item.handoffState === 'failed-dispatch') {
        return 'Retry'
      }
      return 'Dispatch'
    }

    if (!item.track) {
      return 'Review'
    }

    if (experimentalOpenAndPlayEnabled && item.handoffState === 'sent-to-player') {
      return experimentalPlayNextEnabled ? 'Queued' : 'Sent'
    }

    if (item.handoffState === 'failed-dispatch') {
      return 'Retry'
    }

    if (experimentalOpenAndPlayEnabled) {
      return experimentalPlayNextEnabled ? 'Queue next' : 'Play'
    }

    return 'Open'
  }

  const canTriggerHandoff = (item?: QueueItem | null) =>
    controlMode === 'streamer-safe'
      ? Boolean(item) &&
        !['ready-to-send', 'sent-to-player', 'confirmed-playing'].includes(
          item ? item.handoffState : 'pending-match',
        )
      : !experimentalOpenAndPlayEnabled || item?.handoffState !== 'sent-to-player'

  const featuredCanTriggerHandoff =
    controlMode === 'streamer-safe'
      ? Boolean(featuredRequest?.track) &&
        !featuredRequest?.requiresManualReview &&
        ['pending-match', 'ready-to-send', 'failed-dispatch'].includes(featuredRequest.handoffState)
      : Boolean(featuredRequest) && canTriggerHandoff(featuredRequest)

  const featuredActionLabel = featuredRequest ? getQueueActionLabel(featuredRequest) : primaryHandoffLabel
  const requestLinksLabel = state.settings.requestLimits.allowLinks ? 'Allowed' : 'Blocked'
  const degradedNotices = [
    state.storage.warning,
    state.legacyImport.available ? 'Legacy Electron data found. Import it once to migrate settings, queue, and logs.' : null,
    state.probe.lastError ? `Now Playing probe degraded: ${state.probe.lastError}` : null,
    state.botStatus.state === 'error' ? state.botStatus.detail : null,
  ].filter((value): value is string => Boolean(value))

  const openPanel = (panel: PanelKey) => {
    store.setActivePanel(panel)
  }

  const closeModal = () => {
    store.setActivePanel('dashboard')
  }

  const minimizeWindow = () => {
    void invoke('window_minimize')
  }

  const toggleMaximizeWindow = () => {
    void invoke('window_toggle_maximize')
  }

  const closeWindow = () => {
    void invoke('window_close')
  }

  const startWindowDrag = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return
    }
    void invoke('window_start_drag')
  }

  const stopTitleEvent = (event: React.MouseEvent<HTMLButtonElement>) => {
    event.preventDefault()
    event.stopPropagation()
  }

  return (
    <main className={styles.shell}>
      <div className={styles.windowFrame}>
        <div className={styles.titleBar}>
          <div className={styles.titleTopRow}>
            <div className={styles.titleDragLane} onMouseDown={startWindowDrag} onDoubleClick={toggleMaximizeWindow}>
              <div className={styles.titleIdentity}>
                <img className={styles.brandIcon} src={faviconUrl} alt="" />
                <p className={styles.titleInline}>
                  <span className={styles.titleName}>AppleCrap</span>
                  <span className={styles.titleTag}>Portable alpha</span>
                </p>
              </div>
            </div>

            <div className={styles.windowControls}>
              <button
                type="button"
                className={styles.windowControl}
                aria-label="Minimize"
                onMouseDown={stopTitleEvent}
                onDoubleClick={stopTitleEvent}
                onClick={minimizeWindow}
              >
                <span aria-hidden="true">−</span>
              </button>
              <button
                type="button"
                className={styles.windowControl}
                aria-label="Maximize"
                onMouseDown={stopTitleEvent}
                onDoubleClick={stopTitleEvent}
                onClick={toggleMaximizeWindow}
              >
                <span aria-hidden="true">□</span>
              </button>
              <button
                type="button"
                className={cx(styles.windowControl, styles.windowControlClose)}
                aria-label="Close"
                onMouseDown={stopTitleEvent}
                onDoubleClick={stopTitleEvent}
                onClick={closeWindow}
              >
                <span aria-hidden="true">×</span>
              </button>
            </div>
          </div>

          <div className={styles.titleNavRow}>
            <div className={styles.titleMenus}>
              <div className={styles.menuGroup}>
                {utilityPanels.map((panel) => (
                  <button key={panel.key} className={styles.menuTextButton} onClick={() => openPanel(panel.key)}>
                    {panel.label}
                  </button>
                ))}
                <button className={styles.menuTextButton} onClick={() => void invoke('player_show')}>
                  Player ↗
                </button>
              </div>
            </div>
          </div>
        </div>

        <div className={styles.contentViewport}>
          {degradedNotices.length ? (
            <div className={styles.bannerStrip}>
              {degradedNotices.map((message) => (
                <div key={message} className={styles.bannerCard}>
                  <span>{message}</span>
                  {message.includes('Legacy Electron data') ? (
                    <button className={styles.menuTextButton} onClick={store.importLegacy}>
                      Import now
                    </button>
                  ) : null}
                </div>
              ))}
            </div>
          ) : null}

          <section className={styles.mainColumn}>
            <SectionFrame
            title={featuredRequest ? queueHeadline(featuredRequest) : 'Queue is clear'}
            eyebrow="Primary item"
            actions={
              <div className={styles.sectionActionsInline}>
                <button className={styles.ghostButton} onClick={store.rerunProbe}>
                  Refresh probe
                </button>
                <label className={styles.actionToggle}>
                  <input
                    type="checkbox"
                    checked={autoArmEnabled}
                    onChange={(event) => {
                      void store.setAutoArmEnabled(event.target.checked)
                    }}
                  />
                  <span>Auto</span>
                </label>
                {controlMode === 'streamer-safe' && featuredRequest ? (
                  <button className={styles.ghostButton} onClick={store.moveSelectedRequestToManualReview}>
                    Send to review
                  </button>
                ) : null}
                <button
                  className={styles.primaryButton}
                  disabled={!featuredCanTriggerHandoff}
                  onClick={() =>
                    controlMode === 'streamer-safe'
                      ? void store.dispatchReadyRequest()
                      : void store.handoffSelectedTrack(featuredRequest)
                  }
                >
                  {controlMode === 'streamer-safe' ? primaryHandoffLabel : featuredActionLabel}
                </button>
              </div>
            }
          >
            {featuredRequest ? (
              <div className={styles.featureSurface}>
                <div className={styles.featureArt}>
                  {featuredRequest.track?.artworkUrl ? (
                    <img src={featuredRequest.track.artworkUrl} alt="" />
                  ) : (
                    <div className={styles.heroFallback}>On deck</div>
                  )}
                </div>
                <div className={styles.featureBody}>
                  <div className={styles.metaStrip}>
                    <Pill label="Requester" value={`@${featuredRequest.requestedBy}`} />
                    <Pill label="Queue state" value={queueStatusLabel(featuredRequest)} tone={queueStatusTone(featuredRequest)} />
                    <Pill label="Hotkey" value={state.settings.automation.dispatchHotkey} />
                    <Pill label="Auto mode" value={autoArmEnabled ? 'On' : 'Hotkey'} tone={autoArmEnabled ? 'good' : 'neutral'} />
                  </div>
                  <p className={styles.featureSummary}>
                    {featuredRequest.track
                      ? `${featuredRequest.track.albumName || 'Apple Music match'} | ${formatDuration(featuredRequest.track.durationMs)}`
                      : 'Review this request against Apple Music before sending it forward.'}
                  </p>
                  <div className={styles.metaStrip}>
                    <Pill label="Waiting" value={String(waitingRequests)} />
                    <Pill label="Staged" value={String(stagedRequests)} tone={stagedRequests ? 'good' : 'neutral'} />
                    <Pill label="Mode" value={controlMode === 'streamer-safe' ? 'Streamer-safe' : experimentalPlayNextEnabled ? 'Play next' : primaryHandoffLabel} />
                    <Pill label="Confidence" value={featuredRequest.matchConfidence !== null ? `${Math.round(featuredRequest.matchConfidence * 100)}%` : 'Unknown'} />
                  </div>
                  <div className={styles.noticeStrip}>
                    <strong>
                      {controlMode === 'streamer-safe'
                        ? 'Deliberate dispatch mode'
                        : state.probe.matched
                          ? 'Playback confirmed'
                          : 'Waiting for playback confirmation'}
                    </strong>
                    <span>
                      {controlMode === 'streamer-safe'
                        ? featuredRequest.track
                          ? autoArmEnabled
                            ? 'AppleCrap will queue matched requests automatically.'
                            : `Press ${state.settings.automation.dispatchHotkey} when you want Apple Music to queue this request.`
                          : 'Pick an Apple Music match before dispatching this request.'
                        : state.probe.title
                          ? `${state.probe.title} | ${state.probe.artist} (${Math.round(state.probe.confidence * 100)}% confidence)`
                          : 'No compatible Apple Music playback session detected yet.'}
                    </span>
                  </div>
                </div>
              </div>
            ) : (
              <div className={styles.heroEmpty}>
                <p>{emptyStateMessage(requestCommand)}</p>
                <div className={styles.metaStrip}>
                  <Pill label="Command" value={requestCommand} />
                  <Pill label="Queue" value={`${state.stats.totalRequests} request(s)`} />
                  <Pill label="Channel" value={state.botStatus.channel || 'Not configured'} />
                  <Pill label="Auto mode" value={autoArmEnabled ? 'On' : 'Hotkey'} tone={autoArmEnabled ? 'good' : 'neutral'} />
                </div>
              </div>
            )}
            </SectionFrame>

            <SectionFrame
            title="Queue"
            eyebrow="Live lane"
            actions={
              <div className={styles.sectionActionsInline}>
                <button className={styles.ghostButton} onClick={store.wipeQueue}>
                  Clear queue
                </button>
                <button className={styles.ghostButton} onClick={() => openPanel('now-playing')}>
                  Probe
                </button>
              </div>
            }
          >
            <div className={styles.queueHead}>
              <span>Track</span>
              <span>Requester</span>
              <span>Status</span>
              <span>Time</span>
              <span>Actions</span>
            </div>
            <div className={styles.queueBody}>
              {queue.length ? (
                queue.map((item) => (
                  <QueueRow
                    key={item.id}
                    item={item}
                    active={store.selectedRequestId === item.id}
                    openLabel={getQueueActionLabel(item)}
                    openDisabled={
                      controlMode === 'streamer-safe'
                        ? item.handoffState === 'sent-to-player' || item.handoffState === 'confirmed-playing'
                        : !canTriggerHandoff(item)
                    }
                    onSelect={() => store.setSelectedRequestId(item.id)}
                    onOpen={() => store.handoffSelectedTrack(item)}
                    onRemove={() => store.removeQueueItem(item.id)}
                  />
                ))
              ) : (
                <p className={styles.emptyCopy}>No requests are active right now.</p>
              )}
            </div>
            </SectionFrame>

            <SectionFrame
            title="Quick Tools"
            eyebrow="Compact utility strip"
            actions={
              <div className={styles.sectionActionsInline}>
                <button className={styles.ghostButton} onClick={state.botStatus.connected ? store.stopBot : store.startBot}>
                  {state.botStatus.connected ? 'Disconnect bot' : 'Connect bot'}
                </button>
                <button className={styles.ghostButton} onClick={store.openDataFolder}>
                  Reveal data
                </button>
              </div>
            }
          >
            <div className={styles.utilityRow}>
              <div className={styles.compactPane}>
                <p className={styles.eyebrow}>Test request</p>
                <div className={styles.formRow}>
                  <input value={store.manualUser} onChange={(event) => store.setManualUser(event.target.value)} placeholder="viewer" />
                  <input
                    value={store.manualQuery}
                    onChange={(event) => store.setManualQuery(event.target.value)}
                    placeholder="human nature michael jackson"
                  />
                  <button className={styles.secondaryButton} onClick={store.submitManualRequest}>
                    Add
                  </button>
                </div>
              </div>
              <div className={styles.compactPane}>
                <p className={styles.eyebrow}>Resolver</p>
                <div className={styles.formRow}>
                  <input
                    value={store.searchQuery}
                    onChange={(event) => store.setSearchQuery(event.target.value)}
                    placeholder="Search Apple Music"
                  />
                  <button className={styles.secondaryButton} onClick={store.runSearch}>
                    Search
                  </button>
                </div>
                {(store.searchResults?.matches ?? []).length ? (
                  <div className={styles.inlineResultList}>
                    {(store.searchResults?.matches ?? []).slice(0, 4).map((match) => (
                      <button
                        key={match.id}
                        type="button"
                        className={styles.searchResult}
                        onClick={() =>
                          state.controlMode === 'streamer-safe'
                            ? void store.approveSelectedRequest(match)
                            : void store.handoffSelectedTrack(undefined, match.url)
                        }
                      >
                        <strong>{clampText(`${match.title} - ${match.artistName}`, 48)}</strong>
                        <span>{clampText(match.albumName || 'Apple Music result', 56)}</span>
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
            </div>
            </SectionFrame>
          </section>
        </div>
      </div>

      <footer className={styles.statusBar}>
        <span>{store.notice}</span>
        <span>{state.botStatus.detail}</span>
        <span>
          {featuredRequest
            ? `On deck: ${queueHeadline(featuredRequest)} (${queueStatusLabel(featuredRequest)})`
            : `Queue command: ${requestCommand}`}
        </span>
        <span>
          {store.busyAction
            ? `Working: ${store.busyAction}`
            : `Auto mode: ${state.settings.automation.autoArmEnabled ? 'on' : 'hotkey'} | Storage: ${state.storage.mode}`}
        </span>
      </footer>

      {modalPanel === 'bot' ? (
        <ModalShell
          title="Bot Setup"
          eyebrow="Twitch bridge"
          onClose={closeModal}
          actions={
            <>
              <button className={styles.ghostButton} onClick={store.saveDraftSettings}>
                Save
              </button>
              <button className={styles.secondaryButton} onClick={store.startBot}>
                {state.botStatus.connected ? 'Reconnect' : 'Save + connect'}
              </button>
            </>
          }
        >
          <div className={styles.metaStrip}>
            <Pill label="Status" value={state.botStatus.status} tone={state.botStatus.connected ? 'good' : state.botStatus.state === 'error' ? 'warn' : 'neutral'} />
            <Pill label="Listening for" value={requestCommand} />
            <Pill label="Channel" value={state.botStatus.channel || state.settings.twitch.channel || 'Not configured'} />
            <Pill label="Links" value={requestLinksLabel} tone={state.settings.requestLimits.allowLinks ? 'good' : 'warn'} />
          </div>
          <div className={styles.formGrid}>
            <label>
              Twitch channel
              <input value={store.settingsDraft.twitch.channel} onChange={(event) => store.updateDraft('twitch', { channel: event.target.value })} placeholder="kerdylives" />
            </label>
            <label>
              Request command
              <input value={store.settingsDraft.twitch.requestCommand} onChange={(event) => store.updateDraft('twitch', { requestCommand: event.target.value || '!request' })} placeholder="!request" />
            </label>
            <label>
              Bot username
              <input value={store.settingsDraft.twitch.botUsername} onChange={(event) => store.updateDraft('twitch', { botUsername: event.target.value })} placeholder="requestbot" />
            </label>
            <label>
              OAuth token
              <input type="password" value={store.settingsDraft.twitch.oauthToken} onChange={(event) => store.updateDraft('twitch', { oauthToken: event.target.value })} placeholder="oauth:..." />
            </label>
            <label>
              Apple Music storefront
              <input value={store.settingsDraft.appleMusic.storefront} onChange={(event) => store.updateDraft('appleMusic', { storefront: event.target.value || 'us' })} placeholder="us" />
            </label>
            <label className={styles.toggleLabel}>
              <input type="checkbox" checked={store.settingsDraft.twitch.autoConnect} onChange={(event) => store.updateDraft('twitch', { autoConnect: event.target.checked })} />
              Auto-connect on launch
            </label>
          </div>
        </ModalShell>
      ) : null}

      {modalPanel === 'rules' ? (
        <ModalShell title="Queue Rules" eyebrow="Moderation policy" onClose={closeModal} actions={<button className={styles.secondaryButton} onClick={store.saveDraftSettings}>Save rules</button>}>
          <div className={styles.formGrid}>
            <label>
              Max queue size
              <input type="number" min={1} value={store.settingsDraft.requestLimits.maxQueueSize} onChange={(event) => store.updateDraft('requestLimits', { maxQueueSize: Number(event.target.value) })} />
            </label>
            <label>
              Max per user
              <input type="number" min={1} value={store.settingsDraft.requestLimits.maxPerUser} onChange={(event) => store.updateDraft('requestLimits', { maxPerUser: Number(event.target.value) })} />
            </label>
            <label>
              Cooldown seconds
              <input type="number" min={0} value={store.settingsDraft.requestLimits.cooldownSeconds} onChange={(event) => store.updateDraft('requestLimits', { cooldownSeconds: Number(event.target.value) })} />
            </label>
            <label>
              Max track length (minutes)
              <input type="number" min={1} value={store.settingsDraft.requestLimits.maxTrackMinutes} onChange={(event) => store.updateDraft('requestLimits', { maxTrackMinutes: Number(event.target.value) })} />
            </label>
          </div>
          <div className={styles.toggleStack}>
            <label className={styles.toggleLabel}>
              <input type="checkbox" checked={store.settingsDraft.requestLimits.allowDuplicates} onChange={(event) => store.updateDraft('requestLimits', { allowDuplicates: event.target.checked })} />
              Allow duplicate tracks
            </label>
            <label className={styles.toggleLabel}>
              <input type="checkbox" checked={store.settingsDraft.requestLimits.allowLinks} onChange={(event) => store.updateDraft('requestLimits', { allowLinks: event.target.checked })} />
              Allow Apple Music links
            </label>
            <label className={styles.toggleLabel}>
              <input type="checkbox" checked={store.settingsDraft.requestLimits.modsBypassLimits} onChange={(event) => store.updateDraft('requestLimits', { modsBypassLimits: event.target.checked })} />
              Mods bypass limits
            </label>
          </div>
        </ModalShell>
      ) : null}

      {modalPanel === 'automation' ? (
        <ModalShell title="Automation Lab" eyebrow="Reliable + experimental" onClose={closeModal} actions={<button className={styles.secondaryButton} onClick={store.saveDraftSettings}>Save automation</button>}>
          <div className={styles.formGrid}>
            <label>
              Control mode
              <select value={store.settingsDraft.automation.controlMode} onChange={(event) => store.updateDraft('automation', { controlMode: event.target.value as typeof store.settingsDraft.automation.controlMode })}>
                <option value="streamer-safe">Streamer-safe</option>
                <option value="desktop-automation">Desktop automation</option>
              </select>
            </label>
            <label>
              Active adapter
              <select value={store.settingsDraft.automation.adapter} onChange={(event) => store.updateDraft('automation', { adapter: event.target.value as typeof store.settingsDraft.automation.adapter })}>
                <option value="deep-link">Deep link adapter</option>
                <option value="ui-automation">UI automation adapter</option>
              </select>
            </label>
            <label className={styles.toggleLabel}>
              <input type="checkbox" checked={store.settingsDraft.automation.experimentalAutomationEnabled} onChange={(event) => store.updateDraft('automation', { experimentalAutomationEnabled: event.target.checked })} />
              Enable experimental automation bridge
            </label>
            <label className={styles.toggleLabel}>
              <input type="checkbox" checked={store.settingsDraft.automation.autoArmEnabled} onChange={(event) => store.updateDraft('automation', { autoArmEnabled: event.target.checked })} />
              Automatically dispatch matched requests
            </label>
            <label>
              Experimental handoff
              <select value={store.settingsDraft.automation.handoffMode} onChange={(event) => store.updateDraft('automation', { handoffMode: event.target.value as typeof store.settingsDraft.automation.handoffMode })}>
                <option value="play-next">Queue as Play Next</option>
                <option value="play-now">Play immediately</option>
              </select>
            </label>
            <label>
              Dispatch hotkey
              <input value={store.settingsDraft.automation.dispatchHotkey} onChange={(event) => store.updateDraft('automation', { dispatchHotkey: event.target.value })} onBlur={(event) => { void store.updateDispatchHotkey(event.target.value) }} placeholder="F8" />
            </label>
          </div>
          <div className={styles.noteBox}>
            <strong>Current adapter capabilities</strong>
            <p>{store.selectedCapabilities ? `Supports ${store.selectedCapabilities.supportedActions.join(', ')}.` : 'Capabilities will appear after the backend reports them.'}</p>
          </div>
          <div className={styles.noteBox}>
            <strong>{store.settingsDraft.automation.controlMode === 'streamer-safe' ? 'Streamer-safe mode' : 'Desktop automation mode'}</strong>
            <p>{store.settingsDraft.automation.controlMode === 'streamer-safe' ? 'Use the dispatch hotkey or enable Auto when you want AppleCrap to control Apple Music.' : 'This mode may foreground Apple Music and is useful for off-stream testing only.'}</p>
          </div>
          <div className={styles.inlineButtons}>
            <button className={styles.secondaryButton} onClick={() => store.executeAutomation(store.settingsDraft.automation.adapter, 'probe_capabilities')}>Probe capabilities</button>
            <button className={styles.secondaryButton} onClick={() => store.executeAutomation(store.settingsDraft.automation.adapter, 'dry_run')}>Dry run</button>
            <button className={styles.ghostButton} onClick={() => store.executeAutomation(store.settingsDraft.automation.adapter, 'focus_player')}>Focus player</button>
            <button className={styles.ghostButton} onClick={() => store.executeAutomation(store.settingsDraft.automation.adapter, 'attempt_play')}>Attempt play</button>
            <button className={styles.ghostButton} onClick={() => store.executeAutomation(store.settingsDraft.automation.adapter, 'attempt_queue_action')}>Attempt queue action</button>
          </div>
        </ModalShell>
      ) : null}

      {modalPanel === 'now-playing' ? (
        <ModalShell title="Now Playing" eyebrow="Probe state" onClose={closeModal} actions={<button className={styles.secondaryButton} onClick={store.rerunProbe}>Run probe now</button>}>
          <div className={styles.metaStrip}>
            <Pill label="Source" value={state.probe.source || 'idle'} />
            <Pill label="App" value={state.probe.appId || 'Unknown'} />
            <Pill label="Status" value={state.probe.status || 'Stopped'} tone={state.probe.matched ? 'good' : 'neutral'} />
            <Pill label="Confidence" value={`${Math.round(state.probe.confidence * 100)}%`} />
          </div>
          <div className={styles.noteBox}>
            <strong>Match breakdown</strong>
            <p>{state.probe.explanation || 'Waiting for a playback session to compare against the top queue item.'}</p>
          </div>
          <div className={styles.sessionList}>
            {state.probe.sessions.length ? (
              state.probe.sessions.map((session, index) => (
                <article className={styles.sessionCard} key={`${session.appId}-${index}`}>
                  <strong>{session.appId}</strong>
                  <p>{session.title || 'Untitled'}{session.artist ? ` | ${session.artist}` : ''}</p>
                  <span>{session.status || 'Unknown status'}</span>
                </article>
              ))
            ) : (
              <p className={styles.emptyCopy}>No Windows media sessions are visible right now.</p>
            )}
          </div>
        </ModalShell>
      ) : null}

      {modalPanel === 'logs' ? (
        <ModalShell title="Diagnostics Log" eyebrow="Operational visibility" onClose={closeModal} actions={<><button className={styles.ghostButton} onClick={store.copyDebugSummary}>Copy summary</button><button className={styles.secondaryButton} onClick={store.exportLogsAndState}>Export bundle</button></>}>
          <div className={styles.inlineButtons}>
            {(['all', 'info', 'warn', 'error', 'debug'] as LogFilter[]).map((level) => (
              <button key={level} className={cx(styles.filterButton, logFilter === level && styles.filterButtonActive)} onClick={() => setLogFilter(level)}>
                {level}
              </button>
            ))}
          </div>
          <LogList entries={filteredLogs} />
        </ModalShell>
      ) : null}

      {modalPanel === 'about' ? (
        <ModalShell title="About AppleCrap Alpha" eyebrow="Product notes" onClose={closeModal}>
          <div className={styles.noteBox}>
            <strong>What this alpha is</strong>
            <p>A Windows-first Twitch-to-Apple Music handoff desk with a reliable request queue, playback-aware auto-clear, and an experimental automation bridge that can fail safely.</p>
          </div>
          <div className={styles.noteBox}>
            <strong>Portable build intent</strong>
            <p>The app is designed to run from an unzipped folder, prefer a local <code>data</code> directory, and expose clear diagnostics when it has to fall back to user profile storage.</p>
          </div>
        </ModalShell>
      ) : null}

      {modalPanel === 'debug' ? (
        <ModalShell
          title="Debug Tools"
          eyebrow="Support + diagnostics"
          onClose={closeModal}
          actions={
            <>
              <button className={styles.ghostButton} onClick={store.copyDebugSummary}>
                Copy summary
              </button>
              <button className={styles.secondaryButton} onClick={store.exportLogsAndState}>
                Export diagnostics
              </button>
            </>
          }
        >
          <div className={styles.noteBox}>
            <strong>Quick support bundle</strong>
            <p>Copy the summary for chat-sized troubleshooting, or export diagnostics when you want the full state, logs, and runtime details.</p>
          </div>
          <div className={styles.noteBox}>
            <strong>Feedback</strong>
            <p>Open a prefilled email draft to <code>kerdylives@gmail.com</code> with the current debug summary already attached in the body.</p>
          </div>
          <div className={styles.inlineButtons}>
            <button className={styles.secondaryButton} onClick={store.copyDebugSummary}>
              Copy summary
            </button>
            <button className={styles.ghostButton} onClick={store.exportLogsAndState}>
              Export diagnostics
            </button>
            <button className={styles.primaryButton} onClick={store.openFeedbackDraft}>
              Email feedback
            </button>
          </div>
        </ModalShell>
      ) : null}
    </main>
  )
}

export default App
