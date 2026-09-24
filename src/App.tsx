import { useState } from 'react'
import { AlertStack, Nav, TitleBar } from './components/Chrome'
import { Desk } from './components/Desk'
import { Activity, FirstRun, Help, Overlay } from './components/Pages'
import { Setup } from './components/Setup'
import type { AppState } from './types'
import s from './ui.module.css'
import { useAppStore, type AppStore } from './useAppStore'

const SETUP_SKIPPED_KEY = 'applecrap.firstRunDone'

// A brand-new install: no way to reach Twitch chat has been set up yet.
function needsSetup(state: AppState) {
  return !state.auth.broadcaster && !state.auth.bot && !state.settings.twitch.oauthToken
}

function readSetupDone() {
  try {
    return window.localStorage.getItem(SETUP_SKIPPED_KEY) === '1'
  } catch {
    return false
  }
}

function Main({ store, state }: { store: AppStore; state: AppState }) {
  // Decided once, when the app opens: finishing a step mid-setup mustn't
  // yank the guide away.
  const [firstRun, setFirstRun] = useState(() => needsSetup(state) && !readSetupDone())
  const finishFirstRun = () => {
    try {
      window.localStorage.setItem(SETUP_SKIPPED_KEY, '1')
    } catch {
      // Private storage unavailable: the guide simply shows again next time.
    }
    setFirstRun(false)
    store.setView('desk')
  }

  let page
  if (firstRun) {
    page = <FirstRun store={store} state={state} onFinish={finishFirstRun} />
  } else {
    switch (store.view) {
      case 'setup':
        page = <Setup store={store} state={state} />
        break
      case 'overlay':
        page = <Overlay store={store} state={state} />
        break
      case 'activity':
        page = <Activity store={store} state={state} />
        break
      case 'help':
        page = <Help store={store} state={state} />
        break
      default:
        page = <Desk store={store} state={state} />
    }
  }

  return (
    <div className={s.app}>
      <TitleBar state={state} />
      <Nav
        view={firstRun ? 'desk' : store.view}
        onView={(view) => {
          setFirstRun(false)
          store.setView(view)
        }}
        onShowPlayer={store.showPlayer}
      />
      <main className={s.content}>
        <AlertStack store={store} state={state} />
        {page}
      </main>
      <footer className={s.footer}>
        <span className={s.footerNotice} aria-live="polite">
          {store.notice}
        </span>
        {state.botStatus.channel ? <span>#{state.botStatus.channel}</span> : null}
      </footer>
    </div>
  )
}

function App() {
  const store = useAppStore()
  if (!store.state) {
    return <div className={s.loading}>{store.notice}</div>
  }
  return <Main store={store} state={store.state} />
}

export default App
