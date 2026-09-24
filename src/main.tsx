import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ErrorBoundary } from './ErrorBoundary.tsx'

window.addEventListener('error', (event) => {
  console.error('Renderer error', event.error ?? event.message)
})

window.addEventListener('unhandledrejection', (event) => {
  console.error('Renderer rejection', event.reason)
})

function start() {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </StrictMode>,
  )
}

// In a plain browser during development, stand in for the backend so the UI
// can be worked on without the app. Never part of a production build.
if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  void import('./dev/demoBackend').then((demo) => demo.install()).then(start)
} else {
  start()
}
