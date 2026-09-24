import { useState } from 'react'
import type { AppStore } from '../useAppStore'
import type { AppState, AuthSlot } from '../types'
import s from '../ui.module.css'
import { cx } from '../format'

export function SaveState({ store }: { store: AppStore }) {
  const text = {
    idle: 'Changes save automatically',
    pending: 'Saving…',
    saving: 'Saving…',
    saved: 'Saved',
    error: "Couldn't save — check the message below",
  }[store.saveStatus]
  return (
    <span className={s.saveState} aria-live="polite">
      {text}
    </span>
  )
}

const SLOTS: Array<{ slot: AuthSlot; title: string; hint: string }> = [
  { slot: 'broadcaster', title: 'Your channel', hint: 'Chats and runs Channel Points requests' },
  { slot: 'bot', title: 'Bot account', hint: 'Optional — reply as a separate account' },
]

export function TwitchAccounts({ store, state }: { store: AppStore; state: AppState }) {
  const auth = state.auth
  const pending = auth.pending
  const busy = Boolean(store.busyAction)

  return (
    <>
      {auth.error ? (
        <div className={cx(s.alert, s.alertError)}>
          <span className={s.alertText}>{auth.error}</span>
        </div>
      ) : null}
      {pending ? (
        <div className={s.codeBox}>
          <span className={s.hint}>
            On Twitch, enter this code to sign in as {pending.slot === 'bot' ? 'the bot account' : 'your channel'}:
          </span>
          <span className={s.code}>{pending.userCode}</span>
          <span className={s.hint}>This finishes by itself once you approve it on Twitch.</span>
          <div className={s.controls}>
            <button type="button" className={cx(s.btn, s.btnTwitch)} onClick={() => void store.reopenTwitchSignInPage()}>
              Open Twitch
            </button>
            <button type="button" className={cx(s.btn, s.btnGhost)} onClick={() => void store.abortTwitchSignIn()}>
              Cancel
            </button>
          </div>
        </div>
      ) : null}
      <div className={s.card}>
        {SLOTS.map(({ slot, title, hint }) => {
          const account = auth[slot]
          return (
            <div key={slot} className={s.cardRow}>
              <span className={cx(s.avatar, !account && s.avatarEmpty)} aria-hidden="true">
                {account ? account.login.slice(0, 2) : '+'}
              </span>
              <span className={s.rowText}>
                <strong>{account ? `@${account.login}` : title}</strong>
                <span>{account ? title : hint}</span>
              </span>
              {account ? (
                <button type="button" className={cx(s.btn, s.btnSmall, s.btnGhost)} disabled={busy} onClick={() => void store.signOutOfTwitch(slot)}>
                  Sign out
                </button>
              ) : (
                <button
                  type="button"
                  className={cx(s.btn, s.btnSmall, s.btnTwitch)}
                  disabled={busy || Boolean(pending)}
                  onClick={() => void store.startTwitchSignIn(slot)}
                >
                  Sign in
                </button>
              )}
            </div>
          )
        })}
      </div>
    </>
  )
}

function ChatConnection({ store, state }: { store: AppStore; state: AppState }) {
  const bot = state.botStatus
  const tone = bot.state === 'connected' ? s.statusGood : bot.state === 'error' ? s.statusBad : bot.state === 'connecting' ? s.statusWarn : undefined
  const connecting = store.busyAction === 'connect-bot'
  return (
    <div className={s.cardRow} style={{ border: '1px solid var(--line)', borderRadius: 12, background: 'var(--surface)' }}>
      <span className={s.rowText}>
        <strong className={tone}>{bot.status}</strong>
        <span>{bot.detail}</span>
      </span>
      {bot.connected || bot.state === 'connecting' ? (
        <>
          <button type="button" className={cx(s.btn, s.btnSmall)} disabled={connecting} onClick={() => void store.startBot()}>
            Reconnect
          </button>
          <button type="button" className={cx(s.btn, s.btnSmall, s.btnGhost)} onClick={() => void store.stopBot()}>
            Disconnect
          </button>
        </>
      ) : (
        <button type="button" className={cx(s.btn, s.btnSmall, s.btnPrimary)} disabled={connecting} onClick={() => void store.startBot()}>
          {connecting ? 'Connecting…' : 'Connect'}
        </button>
      )}
    </div>
  )
}

const POINTS_STATUS = {
  off: { text: 'Off', tone: undefined },
  starting: { text: 'Starting', tone: s.statusWarn },
  live: { text: 'Live', tone: s.statusGood },
  error: { text: 'Needs attention', tone: s.statusBad },
} as const

function ChannelPoints({ store, state }: { store: AppStore; state: AppState }) {
  const draft = store.settingsDraft.channelPoints
  const status = state.channelPoints
  const phase = POINTS_STATUS[status.phase]
  return (
    <section className={s.section} aria-labelledby="points-heading">
      <div className={s.sectionHead}>
        <span id="points-heading" className={s.label}>
          Channel Points
        </span>
        {draft.enabled || status.phase !== 'off' ? <span className={cx(s.statusText, phase.tone)}>{phase.text}</span> : null}
      </div>
      <div className={cx(s.card, s.cardPad)}>
        <label className={s.check}>
          <input type="checkbox" checked={draft.enabled} onChange={(event) => store.updateDraft('channelPoints', { enabled: event.target.checked })} />
          Take song requests through Channel Points
        </label>
        {draft.enabled ? (
          <>
            {status.detail || !state.auth.broadcaster ? (
              <p className={cx(s.hint, status.phase === 'error' && s.statusBad)}>
                {status.detail || 'Sign in as your channel above first.'}
              </p>
            ) : null}
            <div className={s.grid2}>
              <label className={s.field}>
                Reward name
                <input className={s.input} value={draft.title} maxLength={45} onChange={(event) => store.updateDraft('channelPoints', { title: event.target.value })} />
              </label>
              <label className={s.field}>
                Cost (points)
                <input className={s.input} type="number" min={1} value={draft.cost} onChange={(event) => store.updateDraft('channelPoints', { cost: Number(event.target.value) })} />
              </label>
            </div>
            <label className={s.check}>
              <input type="checkbox" checked={draft.pointsOnly} onChange={(event) => store.updateDraft('channelPoints', { pointsOnly: event.target.checked })} />
              Points only — the chat command points viewers at the reward (mods can still use it)
            </label>
          </>
        ) : (
          <p className={s.hint}>
            Needs a Twitch Affiliate or Partner channel. AppleCrap creates the reward, queues each redemption, and refunds the points
            when a request can't be queued. Turning this off hides the reward.
          </p>
        )}
      </div>
    </section>
  )
}

export function Setup({ store, state }: { store: AppStore; state: AppState }) {
  const [pastedToken, setPastedToken] = useState(false)
  const draft = store.settingsDraft
  const signedIn = Boolean(state.auth.bot || state.auth.broadcaster)
  // The pasted-token fields stay for builds without Twitch sign-in and for
  // setups that already use one; otherwise they hide behind a checkbox.
  const showToken = !state.auth.available || (!signedIn && (pastedToken || Boolean(state.settings.twitch.oauthToken)))
  const limits = draft.requestLimits

  return (
    <div className={s.page}>
      <section className={s.section} aria-labelledby="twitch-heading">
        <div className={s.sectionHead}>
          <span id="twitch-heading" className={s.label}>
            Twitch
          </span>
          <SaveState store={store} />
        </div>
        {state.auth.available ? <TwitchAccounts store={store} state={state} /> : null}
        <div className={s.grid2}>
          <label className={s.field}>
            Channel
            <input className={s.input} value={draft.twitch.channel} onChange={(event) => store.updateDraft('twitch', { channel: event.target.value })} placeholder="yourchannel" />
          </label>
          <label className={s.field}>
            Request command
            <input className={s.input} value={draft.twitch.requestCommand} onChange={(event) => store.updateDraft('twitch', { requestCommand: event.target.value })} placeholder="!request" />
          </label>
          {showToken ? (
            <>
              <label className={s.field}>
                Bot username
                <input className={s.input} value={draft.twitch.botUsername} onChange={(event) => store.updateDraft('twitch', { botUsername: event.target.value })} />
              </label>
              <label className={s.field}>
                OAuth token
                <input className={s.input} type="password" value={draft.twitch.oauthToken} onChange={(event) => store.updateDraft('twitch', { oauthToken: event.target.value })} placeholder="oauth:..." />
              </label>
            </>
          ) : null}
        </div>
        <label className={s.check}>
          <input type="checkbox" checked={draft.twitch.autoConnect} onChange={(event) => store.updateDraft('twitch', { autoConnect: event.target.checked })} />
          Connect to chat when AppleCrap opens
        </label>
        {state.auth.available && !signedIn && !state.settings.twitch.oauthToken ? (
          <label className={s.check}>
            <input type="checkbox" checked={pastedToken} onChange={(event) => setPastedToken(event.target.checked)} />
            Use a pasted token instead (advanced)
          </label>
        ) : null}
        <ChatConnection store={store} state={state} />
      </section>

      {state.auth.available ? <ChannelPoints store={store} state={state} /> : null}

      <section className={s.section} aria-labelledby="player-heading">
        <span id="player-heading" className={s.label}>
          Player
        </span>
        <div className={s.grid2}>
          <label className={s.field}>
            Audio output
            <select className={s.select} value={state.settings.player.audioOutputDevice} onChange={(event) => void store.setAudioOutputDevice(event.target.value)}>
              <option value="">System default</option>
              {state.probe.outputDevices
                .filter((device) => device.id !== '')
                .map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.label}
                  </option>
                ))}
              {state.settings.player.audioOutputDevice &&
              !state.probe.outputDevices.some((device) => device.id === state.settings.player.audioOutputDevice) ? (
                <option value={state.settings.player.audioOutputDevice}>Saved device (not detected right now)</option>
              ) : null}
            </select>
          </label>
          <label className={s.field}>
            Apple Music country
            <input className={s.input} value={draft.appleMusic.storefront} maxLength={2} onChange={(event) => store.updateDraft('appleMusic', { storefront: event.target.value.toLowerCase() })} placeholder="us" />
          </label>
        </div>
        <label className={s.check}>
          <input type="checkbox" checked={state.settings.player.mediaKeys} onChange={(event) => void store.setMediaKeysEnabled(event.target.checked)} />
          Keyboard media keys control AppleCrap while a track is loaded
        </label>
      </section>

      <section className={s.section} aria-labelledby="rules-heading">
        <span id="rules-heading" className={s.label}>
          Queue rules
        </span>
        <div className={s.grid2}>
          <label className={s.field}>
            Queue size
            <input className={s.input} type="number" min={1} value={limits.maxQueueSize} onChange={(event) => store.updateDraft('requestLimits', { maxQueueSize: Number(event.target.value) })} />
          </label>
          <label className={s.field}>
            Requests per viewer
            <input className={s.input} type="number" min={1} value={limits.maxPerUser} onChange={(event) => store.updateDraft('requestLimits', { maxPerUser: Number(event.target.value) })} />
          </label>
          <label className={s.field}>
            Wait between requests (seconds)
            <input className={s.input} type="number" min={0} value={limits.cooldownSeconds} onChange={(event) => store.updateDraft('requestLimits', { cooldownSeconds: Number(event.target.value) })} />
          </label>
          <label className={s.field}>
            Longest song (minutes)
            <input className={s.input} type="number" min={1} value={limits.maxTrackMinutes} onChange={(event) => store.updateDraft('requestLimits', { maxTrackMinutes: Number(event.target.value) })} />
          </label>
        </div>
        <label className={s.check}>
          <input type="checkbox" checked={limits.allowDuplicates} onChange={(event) => store.updateDraft('requestLimits', { allowDuplicates: event.target.checked })} />
          Allow the same song twice
        </label>
        <label className={s.check}>
          <input type="checkbox" checked={limits.allowLinks} onChange={(event) => store.updateDraft('requestLimits', { allowLinks: event.target.checked })} />
          Allow Apple Music links
        </label>
        <label className={s.check}>
          <input type="checkbox" checked={limits.modsBypassLimits} onChange={(event) => store.updateDraft('requestLimits', { modsBypassLimits: event.target.checked })} />
          Mods skip these limits
        </label>
      </section>
    </div>
  )
}
