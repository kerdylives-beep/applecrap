import { execFile } from 'node:child_process'

const POWERSHELL_SCRIPT = String.raw`
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

try {
  Add-Type -AssemblyName UIAutomationClient
  Add-Type -AssemblyName UIAutomationTypes

  function Get-DescendantSnapshots($root) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $items = @()
    foreach ($element in $all) {
      $items += [pscustomobject]@{
        controlType = $element.Current.ControlType.ProgrammaticName
        name = $element.Current.Name
      }
    }
    return $items
  }

  function Get-PlaybackStatus($items) {
    if ($items | Where-Object { $_.controlType -eq 'ControlType.Button' -and $_.name -eq 'Pause' }) {
      return 'Playing'
    }
    if ($items | Where-Object { $_.controlType -eq 'ControlType.Button' -and $_.name -eq 'Play' }) {
      return 'Paused'
    }
    return 'Unknown'
  }

  function Get-SpotifySession {
    $process = Get-Process Spotify -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } |
      Select-Object -First 1

    if (-not $process) {
      return $null
    }

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $items = Get-DescendantSnapshots $root
    $nowPlaying = $items | Where-Object { $_.name -like 'Now playing:* by *' } | Select-Object -First 1

    $title = ''
    $artist = ''
    if ($nowPlaying -and $nowPlaying.name -match '^Now playing:\s*(.+?)\s+by\s+(.+)$') {
      $title = $matches[1].Trim()
      $artist = $matches[2].Trim()
    } elseif ($process.MainWindowTitle -match '^(.*?)\s+-\s+(.*)$') {
      $artist = $matches[1].Trim()
      $title = $matches[2].Trim()
    }

    if (-not $title) {
      return $null
    }

    return [pscustomobject]@{
      appId = 'Spotify'
      status = Get-PlaybackStatus $items
      title = $title
      artist = $artist
      album = ''
    }
  }

  function Get-MediaPlayerSession {
    $process = Get-Process ApplicationFrameHost -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -eq 'Media Player' } |
      Select-Object -First 1

    if (-not $process) {
      return $null
    }

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $items = Get-DescendantSnapshots $root

    $nowPlayingIndex = -1
    for ($index = 0; $index -lt $items.Count; $index++) {
      if ($items[$index].name -like '*. Now playing.' -or $items[$index].name -like '*. Now playing') {
        $nowPlayingIndex = $index
        break
      }
    }

    if ($nowPlayingIndex -lt 0) {
      return $null
    }

    $textItems = @()
    for ($index = $nowPlayingIndex + 1; $index -lt [Math]::Min($items.Count, $nowPlayingIndex + 10); $index++) {
      if ($items[$index].controlType -eq 'ControlType.Text' -and $items[$index].name) {
        $textItems += $items[$index].name
      }
    }

    if ($textItems.Count -eq 0) {
      return $null
    }

    return [pscustomobject]@{
      appId = 'Media Player'
      status = Get-PlaybackStatus $items
      title = $textItems[0]
      artist = if ($textItems.Count -ge 2) { $textItems[1] } else { '' }
      album = ''
    }
  }

  function Get-AppleMusicSession {
    $process = Get-Process AppleMusic -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 } |
      Select-Object -First 1

    if (-not $process) {
      return $null
    }

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $items = Get-DescendantSnapshots $root
    $status = Get-PlaybackStatus $items
    $dash = [string][char]0x2014

    $groups = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($group in $groups) {
      if ($group.Current.ControlType.ProgrammaticName -ne 'ControlType.Group') {
        continue
      }

      $children = $group.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
      $textItems = @()
      foreach ($child in $children) {
        if ($child.Current.ControlType.ProgrammaticName -eq 'ControlType.Text' -and $child.Current.Name) {
          $textItems += $child.Current.Name
        }
      }

      if ($textItems.Count -lt 2) {
        continue
      }

      $title = $textItems[0].Trim()
      $artist = ''
      $album = ''
      $meta = $textItems[1] -replace $dash, ' - '
      if ($meta -match '^(.*?)\s+-\s+(.+)$') {
        $artist = $matches[1].Trim()
        $album = $matches[2].Trim()
      }

      if ($title -and $artist) {
        return [pscustomobject]@{
          appId = 'Apple Music'
          status = $status
          title = $title
          artist = $artist
          album = $album
        }
      }
    }

    return $null
  }

  $sessions = @()
  foreach ($detector in @('Get-SpotifySession', 'Get-MediaPlayerSession', 'Get-AppleMusicSession')) {
    $session = & $detector
    if ($session) {
      $sessions += $session
    }
  }

  $chosen = $sessions | Where-Object { $_.appId -eq 'Apple Music' -and $_.status -eq 'Playing' } | Select-Object -First 1
  if (-not $chosen) {
    $chosen = $sessions | Where-Object { $_.status -eq 'Playing' } | Select-Object -First 1
  }
  if (-not $chosen) {
    $chosen = $sessions | Select-Object -First 1
  }

  [pscustomobject]@{
    session = $chosen
    sessions = $sessions
    error = $null
  } | ConvertTo-Json -Depth 6 -Compress
} catch {
  [pscustomobject]@{
    session = $null
    sessions = @()
    error = $_.Exception.Message
  } | ConvertTo-Json -Depth 6 -Compress
}
`

function normalize(text) {
  return (text || '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim()
}

function isLikelyAppleMusic(appId) {
  const value = (appId || '').toLowerCase()
  return value.includes('apple music') || value.includes('applemusic') || value.includes('appleinc.applemusic')
}

function isTrackMatch(current, queueItem) {
  const currentTitle = normalize(current.title)
  const currentArtist = normalize(current.artist)
  const queueTitle = normalize(queueItem.track?.title || queueItem.query)
  const queueArtist = normalize(queueItem.track?.artistName || '')

  if (!currentTitle || !queueTitle) {
    return false
  }

  const currentTitleTerms = new Set(currentTitle.split(' ').filter(Boolean))
  const queueTitleTerms = queueTitle.split(' ').filter(Boolean)
  const overlappingTitleTerms = queueTitleTerms.filter((term) => currentTitleTerms.has(term)).length
  const titleMatches =
    currentTitle.includes(queueTitle) ||
    queueTitle.includes(currentTitle) ||
    overlappingTitleTerms >= Math.max(2, Math.ceil(queueTitleTerms.length * 0.5))

  const artistMatches =
    !queueArtist ||
    currentArtist.includes(queueArtist) ||
    queueArtist.includes(currentArtist) ||
    queueArtist.split(' ').some((term) => term.length > 2 && currentArtist.includes(term))

  return titleMatches && artistMatches
}

export class NowPlayingService {
  constructor(stateManager) {
    this.stateManager = stateManager
    this.interval = null
    this.lastConfirmedQueueId = null
    this.lastSessionSignature = ''
    this.lastProbeError = ''
    this.current = {
      source: 'idle',
      appId: '',
      status: 'Unknown',
      title: '',
      artist: '',
      album: '',
      matchedQueueId: null,
      matched: false,
      debugSessions: [],
    }
  }

  start() {
    if (this.interval) {
      return
    }

    this.refresh()
    this.interval = setInterval(() => {
      this.refresh()
    }, 2500)
  }

  stop() {
    if (this.interval) {
      clearInterval(this.interval)
      this.interval = null
    }
  }

  async refresh() {
    try {
      const probe = await this.getCurrentSession()
      const probeError = probe?.error || ''
      const current = probe?.session ?? null
      const topItem = this.stateManager.state.queue[0] ?? null
      const matched = Boolean(current && topItem && current.status === 'Playing' && isTrackMatch(current, topItem))

      this.current = current
        ? {
            source: isLikelyAppleMusic(current.appId) ? 'apple-music' : 'other-media',
            appId: current.appId,
            status: current.status,
            title: current.title,
            artist: current.artist,
            album: current.album,
            matchedQueueId: matched ? topItem.id : null,
            matched,
            debugSessions: probe?.sessions ?? [],
          }
        : probeError
          ? {
              source: 'error',
              appId: '',
              status: 'Unavailable',
              title: '',
              artist: '',
              album: '',
              matchedQueueId: null,
              matched: false,
              debugSessions: probe?.sessions ?? [],
            }
          : {
              source: 'idle',
              appId: '',
              status: 'Stopped',
              title: '',
              artist: '',
              album: '',
              matchedQueueId: null,
              matched: false,
              debugSessions: probe?.sessions ?? [],
            }

      if (matched && topItem.id !== this.lastConfirmedQueueId) {
        this.lastConfirmedQueueId = topItem.id
        this.stateManager.addLog('info', `Now playing matched "${topItem.track?.title || topItem.query}". Removing it from the queue.`)
        this.stateManager.removeRequest(topItem.id)
      }

      if (!matched) {
        this.lastConfirmedQueueId = null
      }

      const signature = JSON.stringify(probe?.sessions ?? [])
      if (signature !== this.lastSessionSignature) {
        this.lastSessionSignature = signature
        const summary = (probe?.sessions ?? [])
          .map((session) => `${session.status}:${session.title || 'Unknown'}:${session.artist || 'Unknown'}:${session.appId || 'Unknown app'}`)
          .join(' | ')
        this.stateManager.addLog('info', `Now Playing sessions: ${summary || 'none detected'}`)
      }

      if (probeError && probeError !== this.lastProbeError) {
        this.lastProbeError = probeError
        this.stateManager.addLog('warn', `Now Playing unavailable: ${probeError}`)
      }

      if (!probeError) {
        this.lastProbeError = ''
      }

      this.stateManager.setNowPlaying(this.current)
    } catch (error) {
      this.current = {
        source: 'error',
        appId: '',
        status: 'Unavailable',
        title: '',
        artist: '',
        album: '',
        matchedQueueId: null,
        matched: false,
        debugSessions: [],
      }
      this.stateManager.setNowPlaying(this.current)
      const shortMessage = error?.message ? error.message.split(/\r?\n/)[0] : 'Unknown error'
      if (shortMessage !== this.lastProbeError) {
        this.lastProbeError = shortMessage
        this.stateManager.addLog('warn', `Now Playing probe failed: ${shortMessage}`)
      }
    }
  }

  getCurrentSession() {
    return new Promise((resolve, reject) => {
      const encodedScript = Buffer.from(POWERSHELL_SCRIPT, 'utf16le').toString('base64')
      execFile(
        'powershell.exe',
        ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', encodedScript],
        { windowsHide: true, timeout: 10000 },
        (error, stdout, stderr) => {
          if (error) {
            reject(new Error(stderr?.trim() || `PowerShell exited with code ${error.code ?? 'unknown'}`))
            return
          }

          const output = stdout.trim()
          if (!output) {
            resolve(null)
            return
          }

          try {
            resolve(JSON.parse(output))
          } catch (parseError) {
            reject(new Error(`Unable to parse now playing probe output: ${parseError.message}`))
          }
        },
      )
    })
  }
}
