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

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
)
