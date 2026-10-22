import { useEffect, useRef, useState } from 'react'
import type { AppStore } from '../useAppStore'
import type { AppState } from '../types'
import s from '../ui.module.css'
import { cx } from '../format'

const CHANNEL_KEY = 'applecrap.listenChannel'

const PHASES = {
  off: { text: 'Off', tone: undefined },
  connecting: { text: 'Connecting', tone: s.statusWarn },
  waiting: { text: 'Waiting for a song', tone: s.statusWarn },
  playing: { text: 'Listening', tone: s.statusGood },
  error: { text: 'Needs attention', tone: s.statusBad },
} as const

function readChannel() {
  try {
    return window.localStorage.getItem(CHANNEL_KEY) ?? ''
  } catch {
    return ''
  }
}

// Listen together, the listener's side: follow a streamer's "Now playing"
// posts and play each song here, lined up with their stream.
export function Listen({ store, state }: { store: AppStore; state: AppState }) {
  const listen = state.listen
  const on = listen.phase !== 'off'
  const [channel, setChannel] = useState(() => listen.channel || readChannel())
  const [offsetMs, setOffsetMs] = useState(listen.offsetMs)
  const phase = PHASES[listen.phase] ?? PHASES.off

  // The slider moves freely; the app hears about it once it settles.
  const sentOffset = useRef(listen.offsetMs)
  useEffect(() => {
    if (!on || offsetMs === sentOffset.current) {
      return
    }
    const timer = window.setTimeout(() => {
      sentOffset.current = offsetMs
      void store.setListenOffset(offsetMs)
    }, 350)
    return () => window.clearTimeout(timer)
  }, [offsetMs, on, store])

  const start = () => {
    try {
      window.localStorage.setItem(CHANNEL_KEY, channel.trim())
    } catch {
      // Private storage unavailable: the channel isn't remembered.
    }
    sentOffset.current = offsetMs
    void store.startListening(channel, offsetMs)
  }

  return (
    <div className={s.page}>
      <section className={s.section} aria-labelledby="listen-heading">
        <div className={s.sectionHead}>
          <span id="listen-heading" className={s.label}>
            Listen together
          </span>
          {on || listen.phase === 'error' ? <span className={cx(s.statusText, phase.tone)}>{phase.text}</span> : null}
        </div>
        <p className={s.hint}>
          You found it! Streaming with someone? Play the same music at the same moment: follow the channel their AppleCrap posts songs
          in (<code>!listen</code> in their chat says which), and each song they play starts here too, on your own Apple Music. Sign in to
          Apple Music in the player first. Your song requests wait in your queue while you listen.
        </p>
        <div className={cx(s.card, s.cardPad)}>
          <div className={s.grid2}>
            <label className={s.field}>
              Channel
              <input
                className={s.input}
                value={channel}
                disabled={on}
                placeholder="channel"
                onChange={(event) => setChannel(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && !on && channel.trim()) {
                    start()
                  }
                }}
              />
            </label>
            <div className={s.field} style={{ justifyContent: 'flex-end' }}>
              {on ? (
                <button type="button" className={s.btn} onClick={() => void store.stopListening()}>
                  Stop listening
                </button>
              ) : (
                <button type="button" className={cx(s.btn, s.btnPrimary)} disabled={!channel.trim() || Boolean(store.busyAction)} onClick={start}>
                  Start listening
                </button>
              )}
            </div>
          </div>
          {listen.detail ? <p className={cx(s.hint, listen.phase === 'error' && s.statusBad)}>{listen.detail}</p> : null}
          {on && listen.title ? (
            <div className={s.rowText}>
              <strong>{listen.title}</strong>
              <span>
                {listen.artist}
                {listen.requestedBy ? ` · requested by @${listen.requestedBy}` : ''}
              </span>
            </div>
          ) : null}
        </div>
      </section>

      <section className={s.section} aria-labelledby="offset-heading">
        <span id="offset-heading" className={s.label}>
          Fine-tune
        </span>
        <div className={cx(s.card, s.cardPad)}>
          <label className={s.field}>
            {offsetMs === 0
              ? 'In step with the host'
              : `${(Math.abs(offsetMs) / 1000).toFixed(1)} seconds ${offsetMs > 0 ? 'later' : 'earlier'} than the host`}
            <input
              className={s.range}
              type="range"
              min={-3000}
              max={3000}
              step={100}
              value={offsetMs}
              onChange={(event) => setOffsetMs(Number(event.target.value))}
              onDoubleClick={() => setOffsetMs(0)}
            />
          </label>
          <p className={s.hint}>
            Songs start in step with the host on their own. If you hear yours ahead of theirs on the call, slide right; behind, slide
            left. It applies to the song playing now. Double-click to reset.
          </p>
        </div>
      </section>
    </div>
  )
}
