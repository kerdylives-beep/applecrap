// AppleCrap player bridge. Injected into the embedded music.apple.com window.
// Upstream (page -> Rust): Tauri IPC invoke of `player_bridge_report`; if IPC is
// unavailable for the remote origin, falls back to encoding the payload into
// document.title, which the Rust side polls.
// Downstream (Rust -> page): `window.__ACBRIDGE__.exec(op, id, tag)` via webview eval.
(function () {
  'use strict'

  // Initialization scripts run in every frame; only the top document hosts
  // MusicKit, and child frames must not clobber the status channel.
  if (window.top !== window.self) {
    return
  }

  let ipcBroken = false

  // Debug badge: shows which upstream channel the bridge is using and the
  // last error, so channel failures are visible instead of silent.
  const debugState = { channel: 'starting', sent: 0, lastError: '' }
  let badge = null
  function renderBadge() {
    if (!document.body) {
      return
    }
    if (!badge) {
      badge = document.createElement('div')
      badge.style.cssText =
        'position:fixed;bottom:6px;left:6px;z-index:2147483647;background:rgba(0,0,0,0.8);color:#9f9;font:11px/1.4 Consolas,monospace;padding:4px 8px;border-radius:4px;pointer-events:none;'
      document.body.appendChild(badge)
    }
    badge.textContent =
      'bridge: ' + debugState.channel + ' | sent: ' + debugState.sent +
      (debugState.lastError ? ' | err: ' + debugState.lastError : '')
  }

  function sendViaTitle(payload) {
    try {
      const json = JSON.stringify(payload)
      document.title = 'ACB1|' + btoa(unescape(encodeURIComponent(json)))
      debugState.channel = 'title-fallback'
      debugState.sent += 1
    } catch (error) {
      debugState.lastError = 'title: ' + (error && error.message ? error.message : error)
    }
    renderBadge()
  }

  function report(payload) {
    payload.at = Date.now()
    if (!ipcBroken) {
      try {
        const internals = window.__TAURI_INTERNALS__
        if (internals && typeof internals.invoke === 'function') {
          internals
            .invoke('player_bridge_report', { payload })
            .then(() => {
              debugState.channel = 'ipc'
              debugState.sent += 1
              renderBadge()
            })
            .catch((error) => {
              ipcBroken = true
              debugState.lastError =
                'ipc: ' + (error && error.message ? error.message : String(error))
              sendViaTitle(payload)
            })
          return
        }
        debugState.lastError = 'ipc: __TAURI_INTERNALS__ missing'
      } catch (error) {
        debugState.lastError =
          'ipc threw: ' + (error && error.message ? error.message : String(error))
      }
      ipcBroken = true
    }
    sendViaTitle(payload)
  }

  function instance() {
    const mk = window.MusicKit
    if (!mk || typeof mk.getInstance !== 'function') {
      return null
    }
    try {
      return mk.getInstance() || null
    } catch (_) {
      return null
    }
  }

  function playbackStateName(music) {
    try {
      const mk = window.MusicKit
      const name = mk.PlaybackStates && mk.PlaybackStates[music.playbackState]
      if (typeof name === 'string') {
        return name
      }
    } catch (_) {
      /* fall through */
    }
    return String(music.playbackState)
  }

  function snapshot() {
    const music = instance()
    if (!music) {
      return { kind: 'status', ready: false }
    }

    const item = music.nowPlayingItem || null
    let catalogId = null
    try {
      const params =
        (item && item.attributes && item.attributes.playParams) ||
        (item && item.playParams) ||
        null
      if (params) {
        catalogId = params.catalogId || (params.kind === 'song' && !params.isLibrary ? params.id : null)
      }
    } catch (_) {
      /* leave null */
    }

    return {
      kind: 'status',
      ready: true,
      authorized: !!music.isAuthorized,
      playbackState: playbackStateName(music),
      title: item ? item.title || '' : '',
      artist: item ? item.artistName || '' : '',
      album: item ? item.albumName || '' : '',
      catalogId: catalogId ? String(catalogId) : null,
      itemId: item && item.id ? String(item.id) : null,
      durationMs:
        item && item.playbackDuration ? Math.round(item.playbackDuration * 1000) : null,
    }
  }

  function isPlaying(music) {
    try {
      return window.MusicKit.PlaybackStates[music.playbackState] === 'playing'
    } catch (_) {
      return false
    }
  }

  window.__ACBRIDGE__ = {
    async exec(op, id, tag) {
      const done = (ok, detail) =>
        report({ kind: 'result', tag, ok, detail: String(detail || '') })
      try {
        const music = instance()
        if (!music) {
          return done(false, 'MusicKit is not ready yet.')
        }
        if (!music.isAuthorized) {
          return done(false, 'Not signed in to Apple Music. Open the player window and sign in.')
        }

        switch (op) {
          case 'queueNext':
            await music.playNext({ song: id })
            if (!isPlaying(music)) {
              try {
                await music.play()
              } catch (_) {
                return done(true, 'Queued next; press play in the player to start audio.')
              }
            }
            return done(true, 'Queued to play next.')
          case 'queueLater':
            await music.playLater({ song: id })
            return done(true, 'Queued at the end of Playing Next.')
          case 'playNow':
            await music.setQueue({ song: id, startPlaying: true })
            return done(true, 'Playing now.')
          case 'play':
            await music.play()
            return done(true, 'Playback resumed.')
          case 'pause':
            music.pause()
            return done(true, 'Playback paused.')
          case 'skip':
            await music.skipToNextItem()
            return done(true, 'Skipped to the next item.')
          default:
            return done(false, 'Unknown bridge operation: ' + op)
        }
      } catch (error) {
        return done(false, error && error.message ? error.message : error)
      }
    },
  }

  let eventsAttached = false
  function attachEvents() {
    if (eventsAttached) {
      return
    }
    const music = instance()
    if (!music || typeof music.addEventListener !== 'function') {
      return
    }
    try {
      music.addEventListener('nowPlayingItemDidChange', () => report(snapshot()))
      music.addEventListener('playbackStateDidChange', () => report(snapshot()))
      eventsAttached = true
    } catch (_) {
      /* retry on next tick */
    }
  }

  setInterval(() => {
    attachEvents()
    report(snapshot())
  }, 2000)
})();
