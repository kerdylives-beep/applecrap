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
  const [delayMs, setDelayMs] = useState(listen.delayMs)
  const phase = PHASES[listen.phase] ?? PHASES.off

  // The slider moves freely; the app hears about it once it settles.
  const sentDelay = useRef(listen.delayMs)
  useEffect(() => {
    if (!on || delayMs === sentDelay.current) {
      return
    }
    const timer = window.setTimeout(() => {
      sentDelay.current = delayMs
      void store.setListenDelay(delayMs)
    }, 350)
    return () => window.clearTimeout(timer)
  }, [delayMs, on, store])

  const start = () => {
    try {
      window.localStorage.setItem(CHANNEL_KEY, channel.trim())
    } catch {
      // Private storage unavailable: the channel isn't remembered.
    }
    sentDelay.current = delayMs
    void store.startListening(channel, delayMs)
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
          You found it! Hear a streamer&rsquo;s music in full quality on your own Apple Music, in time with their stream. It works with
          channels running AppleCrap that post each new song in chat. Sign in to Apple Music in the player first.
        </p>
        <div className={cx(s.card, s.cardPad)}>
          <div className={s.grid2}>
            <label className={s.field}>
              Channel
              <input
                className={s.input}
                value={channel}
                disabled={on}
                placeholder="streamer"
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

      <section className={s.section} aria-labelledby="delay-heading">
        <span id="delay-heading" className={s.label}>
          Stream delay
        </span>
        <div className={cx(s.card, s.cardPad)}>
          <label className={s.field}>
            {(delayMs / 1000).toFixed(1)} seconds behind the streamer
            <input
              className={s.range}
              type="range"
              min={0}
              max={30000}
              step={250}
              value={delayMs}
              onChange={(event) => setDelayMs(Number(event.target.value))}
            />
          </label>
          <p className={s.hint}>
            Song ahead of the stream? Slide right. Behind it? Slide left. Changes apply to the song playing now. Twitch&rsquo;s low-latency
            mode is usually 2&ndash;5 seconds.
          </p>
        </div>
      </section>
    </div>
  )
}
