import { memo, useEffect, useState, type FormEvent } from 'react'
import type { AppStore } from '../useAppStore'
import type { AppState, LogEntry, ProbeSnapshot, QueueItem } from '../types'
import s from '../ui.module.css'
import { playerConnectionState } from '../utils'
import { activityEntries, cx, formatClock, sourceLabel } from '../format'
import { Art, CloseIcon, NextIcon, PauseIcon, PlayIcon, PreviousIcon, Switch } from './common'

// ---- Now playing ------------------------------------------------------------

// The track position, advanced locally between player reports (which only
// arrive when something changes).
function usePosition(probe: ProbeSnapshot) {
  const playing = probe.status === 'Playing'
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!playing) {
      return
    }
    const timer = window.setInterval(() => setNow(Date.now()), 500)
    return () => window.clearInterval(timer)
  }, [playing])

  if (probe.positionMs === null || probe.positionMs === undefined) {
    return null
  }
  const anchor = probe.updatedAt ? Date.parse(probe.updatedAt) : now
  const elapsed = playing ? Math.max(0, now - anchor) : 0
  return Math.min(probe.positionMs + elapsed, probe.durationMs ?? Number.POSITIVE_INFINITY)
}

function NowPlaying({ store, state }: { store: AppStore; state: AppState }) {
  const probe = state.probe
  const player = playerConnectionState(probe)
  const position = usePosition(probe)

  if (player === 'sign-in-required') {
    return (
      <section className={s.nowPlaying} aria-label="Now playing">
        <div className={s.npEmpty}>
          <p className={s.npEmptyTitle}>Sign in to Apple Music</p>
          <p className={s.hint}>Open the player and sign in once. It remembers you after that.</p>
          <button type="button" className={cx(s.btn, s.btnPrimary)} onClick={store.showPlayer}>
            Open the player
          </button>
        </div>
      </section>
    )
  }

  if (!probe.title) {
    const starting = player !== 'connected'
    return (
      <section className={s.nowPlaying} aria-label="Now playing">
        <div className={s.npEmpty}>
          <p className={s.npEmptyTitle}>{starting ? 'Starting the player…' : 'Nothing playing'}</p>
          <p className={s.hint}>
            {starting
              ? 'The Apple Music player is loading in the background.'
              : 'Requests start playing on their own. You can also press play in the player.'}
          </p>
          {starting ? null : (
            <button type="button" className={s.btn} onClick={store.showPlayer}>
              Open the player
            </button>
          )}
        </div>
      </section>
    )
  }

  const playing = probe.status === 'Playing'
  const request = state.nowPlayingRequest
  const duration = probe.durationMs ?? null
  const percent = duration && position !== null ? Math.min(100, (position / duration) * 100) : 0
  const busy = store.busyAction?.startsWith('player-') ?? false

  return (
    <section className={s.nowPlaying} aria-label="Now playing">
      <span className={cx(s.label, s.labelAccent)}>Now playing</span>
      <div className={s.npRow}>
        <Art url={probe.artworkUrl} title={probe.title} className={s.npArt} />
        <div className={s.npMeta}>
          <h1 className={s.npTitle}>{probe.title}</h1>
          <span className={s.npSub}>{[probe.artist, probe.album].filter(Boolean).join(' · ')}</span>
          {request ? (
            <span className={s.npRequester}>
              Requested by <span className={s.who}>@{request.requestedBy}</span>
              {request.source === 'channel-points' ? (
                <span className={cx(s.chip, s.chipTwitch)}>Channel Points</span>
              ) : null}
            </span>
          ) : null}
        </div>
      </div>
      {duration ? (
        <div className={s.progress}>
          <div
            className={s.progressTrack}
            role="progressbar"
            aria-label="Track progress"
            aria-valuemin={0}
            aria-valuemax={Math.round(duration / 1000)}
            aria-valuenow={position !== null ? Math.round(position / 1000) : undefined}
          >
            <div className={s.progressFill} style={{ width: `${percent}%` }} />
          </div>
          <div className={s.progressTimes}>
            <span>{formatClock(position)}</span>
            <span>{formatClock(duration)}</span>
          </div>
        </div>
      ) : null}
      <div className={s.controls}>
        <button type="button" className={s.iconBtn} aria-label="Previous" disabled={busy} onClick={() => void store.playerControl('previous')}>
          <PreviousIcon />
        </button>
        <button
          type="button"
          className={cx(s.iconBtn, s.iconBtnPrimary)}
          aria-label={playing ? 'Pause' : 'Play'}
          disabled={busy}
          onClick={() => void store.playerControl('togglePlayPause')}
        >
          {playing ? <PauseIcon /> : <PlayIcon />}
        </button>
        <button type="button" className={s.iconBtn} aria-label="Skip" disabled={busy} onClick={() => void store.playerControl('skip')}>
          <NextIcon />
        </button>
        {probe.bitrate ? <span className={s.controlsNote}>{probe.bitrate} kbps</span> : null}
      </div>
    </section>
  )
}

// ---- Queue ------------------------------------------------------------------

function needsReview(item: QueueItem) {
  return item.requiresManualReview || !item.track || item.handoffState === 'manual-review'
}

function rowSub(item: QueueItem): { text: string; tone?: 'warn' | 'good' } {
  if (item.handoffState === 'failed-dispatch') {
    return { text: item.handoffNote || "Couldn't send it to the player", tone: 'warn' }
  }
  if (needsReview(item)) {
    return { text: 'No match yet — pick one or remove it', tone: 'warn' }
  }
  const detail = [item.track?.artistName, item.track?.albumName].filter(Boolean).join(' · ')
  if (item.handoffState === 'sent-to-player') {
    return { text: `In the player's queue · ${detail}`, tone: 'good' }
  }
  return { text: detail }
}

const QueueRow = memo(function QueueRow({
  item,
  position,
  reviewing,
  playNext,
  onReview,
  onRemove,
  onPlayNext,
}: {
  item: QueueItem
  position: number
  reviewing: boolean
  playNext: boolean
  onReview: (id: string) => void
  onRemove: (id: string) => void
  onPlayNext: () => void
}) {
  const sub = rowSub(item)
  const review = needsReview(item)
  const title = item.track?.title ?? `“${item.query}”`
  return (
    <li className={cx(s.row, (review || reviewing) && s.rowReview)}>
      <span className={s.rowNum}>{position}</span>
      <Art url={item.track?.artworkUrl} title={item.track?.title ?? item.query} className={s.rowArt} />
      <div className={s.rowMain}>
        <div className={s.rowTitle}>{title}</div>
        <div className={cx(s.rowSub, sub.tone === 'warn' && s.rowSubWarn, sub.tone === 'good' && s.rowSubGood)}>{sub.text}</div>
      </div>
      <div className={s.rowWho}>
        <span className={s.rowWhoName}>@{item.requestedBy}</span>
        <span className={s.rowWhoMeta}>
          {sourceLabel(item.source)}
          {item.track?.durationMs ? ` · ${formatClock(item.track.durationMs)}` : ''}
        </span>
      </div>
      <div className={s.rowActions}>
        {playNext ? (
          <button type="button" className={cx(s.btn, s.btnSmall, s.btnPrimary)} onClick={onPlayNext}>
            {item.handoffState === 'failed-dispatch' ? 'Retry' : 'Play next'}
          </button>
        ) : null}
        {review && !reviewing ? (
          <button type="button" className={cx(s.btn, s.btnSmall, s.btnWarn)} onClick={() => onReview(item.id)}>
            Review
          </button>
        ) : null}
        <button type="button" className={s.removeBtn} aria-label={`Remove ${title}`} onClick={() => onRemove(item.id)}>
          <CloseIcon />
        </button>
      </div>
    </li>
  )
})

function ReviewPanel({ store }: { store: AppStore }) {
  const submit = (event: FormEvent) => {
    event.preventDefault()
    void store.searchFor(store.searchQuery)
  }
  const matches = store.searchResults?.matches ?? []
  const searching = store.busyAction === 'search-apple-music'
  return (
    <li className={s.review}>
      <form className={s.reviewSearch} onSubmit={submit}>
        <label className={s.srOnly} htmlFor="review-search">
          Search Apple Music
        </label>
        <input
          id="review-search"
          className={s.input}
          value={store.searchQuery}
          onChange={(event) => store.setSearchQuery(event.target.value)}
          placeholder="Song name and artist"
          autoFocus
        />
        <button type="submit" className={s.btn} disabled={searching}>
          {searching ? 'Searching…' : 'Search'}
        </button>
        <button type="button" className={cx(s.btn, s.btnGhost)} onClick={store.closeReview}>
          Close
        </button>
      </form>
      {matches.length ? (
        <div className={s.results}>
          {matches.slice(0, 5).map((match) => (
            <button key={match.id} type="button" className={s.result} onClick={() => void store.approveReviewedRequest(match)}>
              <Art url={match.artworkUrl} title={match.title} className={s.resultArt} />
              <span className={s.rowMain}>
                <span className={s.rowTitle} style={{ display: 'block' }}>
                  {match.title}
                </span>
                <span className={s.rowSub} style={{ display: 'block' }}>
                  {[match.artistName, match.albumName].filter(Boolean).join(' · ')}
                </span>
              </span>
              <span className={s.rowWhoMeta}>{formatClock(match.durationMs)}</span>
            </button>
          ))}
        </div>
      ) : store.searchResults && !searching ? (
        <p className={s.hint}>Nothing found. Try the song name and artist.</p>
      ) : null}
    </li>
  )
}

function Queue({ store, state }: { store: AppStore; state: AppState }) {
  const [confirmClear, setConfirmClear] = useState(false)
  useEffect(() => {
    if (!confirmClear) {
      return
    }
    const timer = window.setTimeout(() => setConfirmClear(false), 3000)
    return () => window.clearTimeout(timer)
  }, [confirmClear])

  const queue = state.queue
  const autoQueue = store.settingsDraft.player.autoQueue
  const front = queue[0]
  const frontCanPlay =
    Boolean(front?.track) &&
    !needsReview(front) &&
    (front.handoffState === 'failed-dispatch' ||
      (!autoQueue && !['sent-to-player', 'confirmed-playing'].includes(front.handoffState)))
  const command = state.settings.twitch.requestCommand || '!request'
  const pointsLive = state.channelPoints.phase === 'live'

  const submit = (event: FormEvent) => {
    event.preventDefault()
    void store.submitManualRequest()
  }

  return (
    <section className={s.queue} aria-label="Queue">
      <div className={s.queueHeader}>
        <span className={s.label} style={{ flex: 1 }}>
          Up next{queue.length ? ` · ${queue.length}` : ''}
        </span>
        <Switch on={autoQueue} label="Auto-queue" onChange={(on) => void store.setAutoQueueEnabled(on)} />
        {queue.length ? (
          <button
            type="button"
            className={cx(s.btn, s.btnSmall, confirmClear ? s.btnWarn : s.btnGhost)}
            onClick={() => {
              if (confirmClear) {
                setConfirmClear(false)
                void store.wipeQueue()
              } else {
                setConfirmClear(true)
              }
            }}
          >
            {confirmClear ? 'Clear all?' : 'Clear'}
          </button>
        ) : null}
      </div>

      {queue.length ? (
        <ol className={s.queueList}>
          {queue.map((item, index) => [
            <QueueRow
              key={item.id}
              item={item}
              position={index + 1}
              reviewing={store.reviewRequest?.id === item.id}
              playNext={index === 0 && frontCanPlay}
              onReview={store.reviewRequestById}
              onRemove={store.removeQueueItemById}
              onPlayNext={() => void store.dispatchFeaturedRequest()}
            />,
            store.reviewRequest?.id === item.id ? <ReviewPanel key={`${item.id}-review`} store={store} /> : null,
          ])}
        </ol>
      ) : (
        <p className={s.queueEmpty}>
          The queue is clear. Viewers can request with <strong>{command}</strong>
          {pointsLive ? (
            <>
              {' '}
              or redeem <strong>{state.settings.channelPoints.title}</strong>
            </>
          ) : null}
          .
        </p>
      )}

      <form className={s.addForm} onSubmit={submit}>
        <label className={s.srOnly} htmlFor="add-song">
          Song to add
        </label>
        <input
          id="add-song"
          className={cx(s.input, s.addSong)}
          value={store.manualQuery}
          onChange={(event) => store.setManualQuery(event.target.value)}
          placeholder="Add a song"
        />
        <label className={s.srOnly} htmlFor="add-for">
          Requested by (optional)
        </label>
        <input
          id="add-for"
          className={cx(s.input, s.addFor)}
          value={store.manualUser}
          onChange={(event) => store.setManualUser(event.target.value)}
          placeholder="for @viewer (optional)"
        />
        <button type="submit" className={s.btn} disabled={store.busyAction === 'manual-request'}>
          Add
        </button>
      </form>
    </section>
  )
}

// ---- Activity -------------------------------------------------------------------

function withHandles(message: string) {
  return message.split(/(@[A-Za-z0-9_]+)/g).map((part, index) =>
    part.startsWith('@') ? (
      <span key={index} className={s.who}>
        {part}
      </span>
    ) : (
      part
    ),
  )
}

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' })

export const ActivityList = memo(function ActivityList({ entries }: { entries: LogEntry[] }) {
  if (!entries.length) {
    return <p className={s.hint}>Nothing here yet.</p>
  }
  return (
    <ul className={s.activityList}>
      {entries.map((entry) => (
        <li key={entry.id} className={s.activityItem}>
          <span className={s.activityTime}>{timeFormat.format(new Date(entry.timestamp))}</span>
          <span className={cx(entry.level === 'warn' && s.activityWarn, entry.level === 'error' && s.activityError)}>
            {withHandles(entry.message)}
          </span>
        </li>
      ))}
    </ul>
  )
})

// ---- Desk -------------------------------------------------------------------

export function Desk({ store, state }: { store: AppStore; state: AppState }) {
  const recent = activityEntries(state.logs, 'requests').slice(0, 14)
  return (
    <div className={s.desk}>
      <NowPlaying store={store} state={state} />
      <Queue store={store} state={state} />
      <aside className={s.activity} aria-label="Recent activity">
        <div className={s.label} style={{ paddingBottom: 8 }}>
          Activity
        </div>
        <ActivityList entries={recent} />
      </aside>
    </div>
  )
}
