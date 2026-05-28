import path from 'node:path'
import fs from 'node:fs'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { app, BrowserWindow, clipboard, dialog, ipcMain, Menu, shell } from 'electron'
import { AppleMusicService } from './apple-music.mjs'
import { NowPlayingService } from './now-playing.mjs'
import { StateManager } from './state.mjs'
import { TwitchBotService } from './twitch-bot.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const isDev = !app.isPackaged
const devServerUrl = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:5173'
const appId = 'com.kerdylives.applecrap'
const TWITCH_TOKEN_URL = 'https://twitchtokengenerator.com/'

let mainWindow = null

const appleMusicService = new AppleMusicService()
const stateManager = new StateManager(path.join(app.getPath('userData'), 'song-requests.json'), appleMusicService)
const twitchBot = new TwitchBotService(stateManager)
const nowPlayingService = new NowPlayingService(stateManager)

app.setAppUserModelId(appId)

function writeDebugLog(message) {
  try {
    const logPath = path.join(app.getPath('userData'), 'debug.log')
    fs.appendFileSync(logPath, `[${new Date().toISOString()}] ${message}\n`)
  } catch {
    // Ignore logging failures.
  }
}

function broadcastState() {
  if (mainWindow) {
    mainWindow.webContents.send('state:update', stateManager.snapshot())
  }
}

function sendMenuAction(action) {
  if (mainWindow) {
    mainWindow.webContents.send('menu:action', action)
  }
}

function edgeCandidates() {
  const candidates = [
    path.join(process.env['ProgramFiles(x86)'] || '', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
    path.join(process.env.ProgramFiles || '', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
    path.join(process.env.LocalAppData || '', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
  ]

  return candidates.filter(Boolean)
}

function launchEdgeInPrivate(url) {
  const foundPath = edgeCandidates().find((candidate) => fs.existsSync(candidate))
  const command = foundPath || 'msedge'

  return new Promise((resolve, reject) => {
    const child = spawn(command, ['--inprivate', url], {
      detached: true,
      stdio: 'ignore',
      windowsHide: true,
    })

    let settled = false
    child.once('spawn', () => {
      settled = true
      child.unref()
      resolve(true)
    })
    child.once('error', (error) => {
      if (settled) return
      settled = true
      reject(error)
    })
  })
}

function buildAppMenu() {
  const template = [
    {
      label: 'AppleCrap',
      submenu: [
        { label: 'Setup Bot', click: () => sendMenuAction('wizard') },
        { type: 'separator' },
        { label: 'Quit', role: 'quit' },
      ],
    },
    {
      label: 'Queue',
      submenu: [
        { label: 'Rules', click: () => sendMenuAction('rules') },
        { label: 'Clear Queue', click: () => sendMenuAction('clearQueue') },
      ],
    },
    {
      label: 'Tools',
      submenu: [
        { label: 'Testing', click: () => sendMenuAction('testing') },
        { label: 'Now Playing', click: () => sendMenuAction('nowPlaying') },
        { label: 'Activity Log', click: () => sendMenuAction('logs') },
      ],
    },
    {
      label: 'Window',
      role: 'windowMenu',
    },
  ]

  if (isDev) {
    template.push({
      label: 'Developer',
      submenu: [
        { role: 'reload' },
        { role: 'toggleDevTools' },
      ],
    })
  }

  Menu.setApplicationMenu(Menu.buildFromTemplate(template))
}

async function createWindow() {
  mainWindow = new BrowserWindow({
    width: 860,
    height: 820,
    minWidth: 760,
    minHeight: 640,
    backgroundColor: '#0b1218',
    title: 'AppleCrap',
    icon: path.join(__dirname, '..', 'build', 'icons', 'applecrap-icon.ico'),
    titleBarStyle: 'hidden',
    titleBarOverlay: {
      color: '#111921',
      symbolColor: '#edf2ee',
      height: 36,
    },
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  })

  stateManager.on('state', broadcastState)
  mainWindow.webContents.on('console-message', (_, level, message, line, sourceId) => {
    writeDebugLog(`console-message level=${level} source=${sourceId} line=${line} message=${message}`)
  })
  mainWindow.webContents.on('did-fail-load', (_, errorCode, errorDescription, validatedURL) => {
    writeDebugLog(`did-fail-load ${errorCode} ${errorDescription} ${validatedURL}`)
  })
  mainWindow.webContents.on('render-process-gone', (_, details) => {
    writeDebugLog(`render-process-gone ${JSON.stringify(details)}`)
  })
  mainWindow.on('unresponsive', () => {
    writeDebugLog('window unresponsive')
  })

  if (isDev) {
    try {
      await mainWindow.loadURL(devServerUrl)
    } catch (error) {
      await mainWindow.loadURL(`data:text/html,${encodeURIComponent(`
        <html>
          <body style="margin:0;padding:32px;background:#0b1218;color:#eef5eb;font-family:Segoe UI,sans-serif">
            <h1>Renderer failed to load</h1>
            <p>Electron opened, but Vite was not ready at ${devServerUrl}.</p>
            <pre style="white-space:pre-wrap">${String(error)}</pre>
          </body>
        </html>
      `)}`)
    }
  } else {
    await mainWindow.loadFile(path.join(__dirname, '..', 'dist', 'index.html'))
  }

  mainWindow.on('closed', () => {
    writeDebugLog('main window closed')
    mainWindow = null
  })
}

process.on('uncaughtException', (error) => {
  writeDebugLog(`uncaughtException ${error.stack || error}`)
  dialog.showErrorBox('Main Process Crash', String(error.stack || error))
})

process.on('unhandledRejection', (reason) => {
  writeDebugLog(`unhandledRejection ${reason instanceof Error ? reason.stack : String(reason)}`)
})

app.whenReady().then(async () => {
  buildAppMenu()
  ipcMain.handle('app:get-state', () => stateManager.snapshot())
  ipcMain.handle('settings:update', async (_, settings) => {
    const updated = stateManager.updateSettings(settings)
    if (!updated.twitch.autoConnect && twitchBot.isConnected()) {
      await twitchBot.disconnect()
    }
    return stateManager.snapshot()
  })
  ipcMain.handle('bot:start', async () => {
    await twitchBot.connect()
    return stateManager.snapshot()
  })
  ipcMain.handle('bot:stop', async () => {
    await twitchBot.disconnect()
    return stateManager.snapshot()
  })
  ipcMain.handle('queue:create-manual', async (_, payload) => stateManager.createManualRequest(payload.requestedBy, payload.query))
  ipcMain.handle('queue:remove', (_, id) => {
    stateManager.removeRequest(id)
    return stateManager.snapshot()
  })
  ipcMain.handle('queue:clear', () => {
    stateManager.clearQueue()
    return stateManager.snapshot()
  })
  ipcMain.handle('apple-music:search', (_, query) => appleMusicService.searchTracks(query, stateManager.state.settings.appleMusic))
  ipcMain.handle('shell:open-external', (_, url) => shell.openExternal(url))
  ipcMain.handle('tools:copy-text', (_, text) => {
    clipboard.writeText(String(text || ''))
    return true
  })
  ipcMain.handle('tools:open-token-generator-private', async () => {
    try {
      await launchEdgeInPrivate(TWITCH_TOKEN_URL)
      return {
        opened: true,
        copied: false,
        message: 'Opened the Twitch token generator in Edge InPrivate.',
      }
    } catch (error) {
      clipboard.writeText(TWITCH_TOKEN_URL)
      writeDebugLog(`open-token-generator-private failed: ${error.stack || error}`)
      return {
        opened: false,
        copied: true,
        message: 'Edge InPrivate could not be opened. The token link was copied instead.',
      }
    }
  })

  await createWindow()
  nowPlayingService.start()

  if (stateManager.state.settings.twitch.autoConnect) {
    try {
      await twitchBot.connect()
    } catch (error) {
      stateManager.addLog('error', `Auto-connect failed: ${error.message}`)
    }
  }
})

app.on('window-all-closed', async () => {
  nowPlayingService.stop()
  await twitchBot.disconnect()
  if (process.platform !== 'darwin') {
    app.quit()
  }
})

app.on('activate', async () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    await createWindow()
  }
})
