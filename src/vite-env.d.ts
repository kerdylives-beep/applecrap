/// <reference types="vite/client" />

import type { SongRequestAPI } from './types'

declare global {
  interface Window {
    songRequestApp: SongRequestAPI
  }
}
