import { invoke } from '@tauri-apps/api/core'
import type { MouseEvent } from 'react'
import faviconUrl from '../../img/favicon.ico'
import type { AppStore } from '../useAppStore'
import type { AppState, ViewKey } from '../types'
import s from '../ui.module.css'
import { playerConnectionState } from '../utils'
import { cx } from '../format'
import { CloseIcon } from './common'

type Tone = 'good' | 'warn' | 'bad' | 'off'

function Dot({ label, tone, title }: { label: string; tone: Tone; title: string }) {
  return (
    <span
      className={cx(s.dot, tone === 'good' && s.dotGood, tone === 'warn' && s.dotWarn, tone === 'bad' && s.dotBad)}
      title={title}
    >
      {label}
    </span>
  )
}

function statusDots(state: AppState) {
  const player = playerConnectionState(state.probe)
  const dots: Array<{ label: string; tone: Tone; title: string }> = [
    {
      label: 'Player',
      tone: player === 'connected' ? 'good' : player === 'sign-in-required' ? 'warn' : 'off',
      title:
        player === 'connected'
          ? 'Apple Music player is ready'
          : player === 'sign-in-required'
            ? 'Sign in to Apple Music in the player'
            : 'Apple Music player is starting',
    },
    {
      label: 'Chat',
      tone:
        state.botStatus.state === 'connected'
          ? 'good'
          : state.botStatus.state === 'connecting'
            ? 'warn'
            : state.botStatus.state === 'error'
              ? 'bad'
              : 'off',
      title: state.botStatus.detail || state.botStatus.status,
    },
  ]
  if (state.settings.channelPoints.enabled) {
    const phase = state.channelPoints.phase
    dots.push({
      label: 'Points',
      tone: phase === 'live' ? 'good' : phase === 'starting' ? 'warn' : phase === 'error' ? 'bad' : 'off',
      title: state.channelPoints.detail || 'Channel Points requests',
    })
  }
  return dots
}

export function TitleBar({ state }: { state: AppState }) {
  const startDrag = (event: MouseEvent<HTMLElement>) => {
    if (event.button === 0 && !(event.target as HTMLElement).closest('button')) {
      void invoke('window_start_drag')
    }
  }
  const toggleMaximize = (event: MouseEvent<HTMLElement>) => {
    if (!(event.target as HTMLElement).closest('button')) {
      void invoke('window_toggle_maximize')
    }
  }

  return (
    <header className={s.titleBar} onMouseDown={startDrag} onDoubleClick={toggleMaximize}>
      <div className={s.brand}>
        <img className={s.brandIcon} src={faviconUrl} alt="" />
        <span className={s.brandName}>AppleCrap</span>
        <span className={s.brandTag}>Beta</span>
      </div>
      <div className={s.statusDots}>
        {statusDots(state).map((dot) => (
          <Dot key={dot.label} {...dot} />
        ))}
      </div>
      <div className={s.windowControls}>
        <button type="button" className={s.windowControl} aria-label="Minimize" onClick={() => void invoke('window_minimize')}>
          &#8211;
        </button>
        <button type="button" className={s.windowControl} aria-label="Maximize" onClick={() => void invoke('window_toggle_maximize')}>
          &#9633;
        </button>
        <button type="button" className={cx(s.windowControl, s.windowClose)} aria-label="Close" onClick={() => void invoke('window_close')}>
          &#215;
        </button>
      </div>
    </header>
  )
}

const VIEWS: Array<{ key: ViewKey; label: string }> = [
  { key: 'desk', label: 'Desk' },
  { key: 'setup', label: 'Setup' },
  { key: 'overlay', label: 'Overlay' },
  { key: 'activity', label: 'Activity' },
  { key: 'help', label: 'Help' },
]

export function Nav({ view, onView, onShowPlayer }: { view: ViewKey; onView: (view: ViewKey) => void; onShowPlayer: () => void }) {
  return (
    <nav className={s.nav} aria-label="Sections">
      {VIEWS.map((item) => (
        <button
          key={item.key}
          type="button"
          className={cx(s.navItem, view === item.key && s.navItemActive)}
          aria-current={view === item.key ? 'page' : undefined}
          onClick={() => onView(item.key)}
        >
          {item.label}
        </button>
      ))}
      <span className={s.navSpacer} />
      <button type="button" className={s.navItem} onClick={onShowPlayer}>
        Player &#8599;
      </button>
    </nav>
  )
}

type Banner = {
  id: string
  tone: 'info' | 'warn' | 'error'
  title: string
  detail?: string
  actions?: Array<{ label: string; primary?: boolean; busy?: boolean; run: () => void }>
  dismiss?: () => void
}

// Everything the user should know about right now, in one place.
export function AlertStack({ store, state }: { store: AppStore; state: AppState }) {
  const banners: Banner[] = []

  if (state.update) {
    banners.push(
      state.update.rolledBack
        ? {
            id: 'update',
            tone: 'info',
            title: `Version ${state.update.version} is out`,
            detail: "It didn't start on this PC last time, so it won't install automatically. The next version will.",
          }
        : {
            id: 'update',
            tone: 'info',
            title: `Version ${state.update.version} is ready to install`,
            detail: 'Your settings and queue are kept.',
            actions: [
              {
                label: store.busyAction === 'install-update' ? 'Installing…' : 'Install and restart',
                primary: true,
                busy: store.busyAction === 'install-update',
                run: () => void store.installUpdate(),
              },
            ],
          },
    )
  }

  for (const alert of state.alerts) {
    banners.push({
      id: alert.id,
      tone: alert.tone,
      title: alert.title,
      detail: alert.detail,
      actions:
        alert.action?.kind === 'reportProblem'
          ? [{ label: 'Report a problem', run: () => void store.reportProblem(alert.id) }]
          : undefined,
      dismiss: () => void store.dismissAlert(alert.id),
    })
  }

  if (state.storage.warning) {
    banners.push({ id: 'storage', tone: 'warn', title: state.storage.warning })
  }
  if (state.botStatus.state === 'error' && state.botStatus.detail) {
    banners.push({
      id: 'chat',
      tone: 'error',
      title: 'Chat is offline',
      detail: state.botStatus.detail,
      actions: [{ label: 'Open setup', run: () => store.setView('setup') }],
    })
  }
  if (state.probe.lastError) {
    banners.push({ id: 'probe', tone: 'warn', title: state.probe.lastError })
  }
  if (state.legacyImport.available) {
    banners.push({
      id: 'legacy',
      tone: 'info',
      title: 'Settings from the old AppleCrap were found',
      detail: 'Import them once to bring over your settings, queue and logs.',
      actions: [{ label: 'Import', run: () => void store.importLegacy() }],
    })
  }

  if (!banners.length) {
    return null
  }

  return (
    <div className={s.alerts} role="status">
      {banners.map((banner) => (
        <div
          key={banner.id}
          className={cx(s.alert, banner.tone === 'warn' && s.alertWarn, banner.tone === 'error' && s.alertError)}
        >
          <div className={s.alertText}>
            <span className={s.alertTitle}>{banner.title}</span>
            {banner.detail ? <span className={s.alertDetail}>{banner.detail}</span> : null}
          </div>
          <div className={s.alertActions}>
            {banner.actions?.map((action) => (
              <button
                key={action.label}
                type="button"
                className={cx(s.btn, s.btnSmall, action.primary && s.btnPrimary)}
                disabled={action.busy}
                onClick={action.run}
              >
                {action.label}
              </button>
            ))}
            {banner.dismiss ? (
              <button type="button" className={s.removeBtn} aria-label="Dismiss" onClick={banner.dismiss}>
                <CloseIcon />
              </button>
            ) : null}
          </div>
        </div>
      ))}
    </div>
  )
}
