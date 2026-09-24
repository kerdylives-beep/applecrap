import { getVersion } from '@tauri-apps/api/app'
import { useDeferredValue, useEffect, useMemo, useState } from 'react'
import type { AppStore } from '../useAppStore'
import type { AppState } from '../types'
import s from '../ui.module.css'
import { playerConnectionState } from '../utils'
import { activityEntries, cx, type ActivityMode } from '../format'
import { CheckIcon } from './common'
import { ActivityList } from './Desk'
import { SaveState, TwitchAccounts } from './Setup'

// ---- Overlay ----------------------------------------------------------------------

export function Overlay({ store, state }: { store: AppStore; state: AppState }) {
  const draft = store.settingsDraft.overlay
  const url = `http://127.0.0.1:${state.settings.overlay.port}/`
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      store.setNotice("Couldn't reach the clipboard. Select the address and copy it by hand.")
    }
  }

  return (
    <div className={s.page}>
      <section className={s.section}>
        <div className={s.sectionHead}>
          <span className={s.label}>OBS overlay</span>
          <SaveState store={store} />
        </div>
        <p className={s.hint}>
          Add this address as a <strong>Browser</strong> source in OBS. It shows the song playing, who asked for it, and what's
          next. The background is see-through, and it hides itself when nothing is playing.
        </p>
        <div className={s.urlBox}>
          <code>{url}</code>
          <button type="button" className={s.btn} onClick={() => void copy()}>
            {copied ? 'Copied' : 'Copy'}
          </button>
          <button type="button" className={cx(s.btn, s.btnGhost)} onClick={() => void store.openOverlayPreview()}>
            Preview
          </button>
        </div>
        <p className={s.hint}>Size in OBS: 560 × 220 (or 560 × 120 without "next up").</p>
      </section>
      <section className={s.section}>
        <span className={s.label}>Options</span>
        <label className={s.check}>
          <input type="checkbox" checked={draft.enabled} onChange={(event) => store.updateDraft('overlay', { enabled: event.target.checked })} />
          Serve the overlay
        </label>
        <label className={s.check}>
          <input type="checkbox" checked={draft.showQueue} onChange={(event) => store.updateDraft('overlay', { showQueue: event.target.checked })} />
          Show what's next up
        </label>
        <div className={s.grid2}>
          <label className={s.field}>
            Songs in "next up"
            <input className={s.input} type="number" min={1} max={10} value={draft.queueCount} onChange={(event) => store.updateDraft('overlay', { queueCount: Number(event.target.value) })} />
          </label>
          <label className={s.field}>
            Port
            <input className={s.input} type="number" min={1024} max={65535} value={draft.port} onChange={(event) => store.updateDraft('overlay', { port: Number(event.target.value) })} />
          </label>
        </div>
      </section>
    </div>
  )
}

// ---- Activity ---------------------------------------------------------------------

const MODES: Array<{ key: ActivityMode; label: string }> = [
  { key: 'requests', label: 'Requests' },
  { key: 'problems', label: 'Problems' },
  { key: 'all', label: 'Everything' },
]

export function Activity({ store, state }: { store: AppStore; state: AppState }) {
  const [mode, setMode] = useState<ActivityMode>('requests')
  const deferredMode = useDeferredValue(mode)
  const entries = useMemo(() => activityEntries(state.logs, deferredMode), [state.logs, deferredMode])
  return (
    <div className={s.page}>
      <div className={s.sectionHead}>
        <div className={s.filters} role="group" aria-label="Show">
          {MODES.map((item) => (
            <button
              key={item.key}
              type="button"
              className={cx(s.navItem, mode === item.key && s.navItemActive)}
              aria-pressed={mode === item.key}
              onClick={() => setMode(item.key)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <button type="button" className={cx(s.btn, s.btnSmall, s.btnGhost)} onClick={() => void store.copyDebugSummary()}>
          Copy summary
        </button>
      </div>
      <ActivityList entries={entries} />
    </div>
  )
}

// ---- Help -------------------------------------------------------------------------

export function Help({ store, state }: { store: AppStore; state: AppState }) {
  const [version, setVersion] = useState('')
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => undefined)
  }, [])
  const probe = state.probe

  return (
    <div className={s.page}>
      <section className={s.section}>
        <span className={s.label}>Something wrong?</span>
        <div className={cx(s.card, s.cardPad)}>
          <p className={s.hint}>
            Report a problem saves a report (settings, recent activity, no passwords or tokens), shows it in Explorer, and opens an
            email to attach it to.
          </p>
          <div className={s.controls}>
            <button type="button" className={cx(s.btn, s.btnPrimary)} disabled={store.busyAction === 'report-problem'} onClick={() => void store.reportProblem()}>
              Report a problem
            </button>
            <button type="button" className={cx(s.btn, s.btnGhost)} onClick={() => void store.copyDebugSummary()}>
              Copy summary
            </button>
          </div>
        </div>
      </section>

      <section className={s.section}>
        <span className={s.label}>Updates</span>
        <div className={cx(s.card, s.cardRow)}>
          <span className={s.rowText}>
            <strong>AppleCrap {version ? `v${version}` : ''}</strong>
            <span>
              {state.update
                ? `Version ${state.update.version} is available.`
                : 'Updates install from the banner at the top when one is out.'}
            </span>
          </span>
          <button type="button" className={cx(s.btn, s.btnSmall)} disabled={store.busyAction === 'check-updates'} onClick={() => void store.checkForUpdates()}>
            {store.busyAction === 'check-updates' ? 'Checking…' : 'Check now'}
          </button>
        </div>
      </section>

      <section className={s.section}>
        <span className={s.label}>Your data</span>
        <p className={s.hint}>
          Settings and the queue live in the <code>data</code> folder{state.storage.mode === 'portable' ? ' next to the app' : ''}.
          Twitch sign-ins there are encrypted to your Windows account.
        </p>
        <div className={s.controls}>
          <button type="button" className={s.btn} onClick={() => void store.openDataFolder()}>
            Open data folder
          </button>
          <button type="button" className={cx(s.btn, s.btnGhost)} onClick={() => void store.exportLogsAndState()}>
            Save diagnostics
          </button>
        </div>
      </section>

      <details className={s.details}>
        <summary>Player details</summary>
        <div className={s.detailsBody}>
          <span>Status: {probe.status || 'Unknown'} ({playerConnectionState(probe)})</span>
          <span>Quality: {probe.bitrate ? `${probe.bitrate} kbps AAC` : 'Unknown'}</span>
          <span>Match: {Math.round(probe.confidence * 100)}% — {probe.explanation}</span>
          {probe.lastError ? <span>Last problem: {probe.lastError}</span> : null}
        </div>
      </details>

      <p className={s.hint}>
        AppleCrap is in beta: it works, but you may find rough edges. Lossless and Spatial Audio aren't available to any web
        player, so playback is 256 kbps AAC — more than Twitch passes on to viewers anyway.
      </p>
    </div>
  )
}

// ---- First run ---------------------------------------------------------------------

function Step({ number, done, title, children }: { number: number; done: boolean; title: string; children: React.ReactNode }) {
  return (
    <li className={cx(s.step, done && s.stepDone)}>
      <span className={s.stepNum} aria-label={done ? 'Done' : undefined}>
        {done ? <CheckIcon /> : number}
      </span>
      <div className={s.stepBody}>
        <h2>{title}</h2>
        {children}
      </div>
    </li>
  )
}

export function FirstRun({ store, state, onFinish }: { store: AppStore; state: AppState; onFinish: () => void }) {
  const player = playerConnectionState(state.probe)
  const appleDone = player === 'connected'
  const twitchDone = Boolean(state.auth.broadcaster || state.auth.bot || state.settings.twitch.oauthToken)
  const chatDone = state.botStatus.connected
  const allDone = appleDone && twitchDone && chatDone

  return (
    <div className={s.firstRun}>
      <div>
        <h1 className={s.firstRunTitle}>Let's get you set up</h1>
        <p className={s.hint}>Three steps, about two minutes. You only do this once.</p>
      </div>
      <ol className={s.steps}>
        <Step number={1} done={appleDone} title="Sign in to Apple Music">
          <p className={s.hint}>
            {appleDone
              ? 'The player is signed in and ready.'
              : player === 'sign-in-required'
                ? 'Open the player and sign in with your Apple Music account. It remembers you after that.'
                : 'The player is starting up in the background…'}
          </p>
          {appleDone ? null : (
            <div className={s.controls}>
              <button type="button" className={cx(s.btn, player === 'sign-in-required' && s.btnPrimary)} onClick={store.showPlayer}>
                Open the player
              </button>
            </div>
          )}
        </Step>
        <Step number={2} done={twitchDone} title="Sign in with Twitch">
          {state.auth.available ? (
            <>
              <p className={s.hint}>Sign in as your channel. A separate bot account is optional and can be added later.</p>
              <TwitchAccounts store={store} state={state} />
            </>
          ) : (
            <p className={s.hint}>Add your bot's username and token in Setup.</p>
          )}
        </Step>
        <Step number={3} done={chatDone} title="Connect to chat">
          <div className={s.grid2}>
            <label className={s.field}>
              Channel
              <input className={s.input} value={store.settingsDraft.twitch.channel} onChange={(event) => store.updateDraft('twitch', { channel: event.target.value })} placeholder="yourchannel" />
            </label>
            <label className={s.field}>
              Request command
              <input className={s.input} value={store.settingsDraft.twitch.requestCommand} onChange={(event) => store.updateDraft('twitch', { requestCommand: event.target.value })} />
            </label>
          </div>
          <div className={s.controls}>
            <button
              type="button"
              className={cx(s.btn, !chatDone && twitchDone && s.btnPrimary)}
              disabled={!twitchDone || store.busyAction === 'connect-bot'}
              onClick={() => {
                if (!store.settingsDraft.twitch.autoConnect) {
                  store.updateDraft('twitch', { autoConnect: true })
                }
                void store.startBot()
              }}
            >
              {chatDone ? 'Connected' : 'Connect'}
            </button>
            <span className={s.hint}>{state.botStatus.detail}</span>
          </div>
        </Step>
      </ol>
      <div className={s.controls}>
        <button type="button" className={cx(s.btn, allDone && s.btnPrimary)} onClick={onFinish}>
          {allDone ? "Done — let's go" : 'Finish later'}
        </button>
      </div>
    </div>
  )
}
