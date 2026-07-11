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

  function sendViaTitle(payload) {
    try {
      const json = JSON.stringify(payload)
      document.title = 'ACB1|' + btoa(unescape(encodeURIComponent(json)))
    } catch (_) {
      /* nothing else we can do if both channels fail */
    }
  }

  function report(payload) {
    payload.at = Date.now()
    if (!ipcBroken) {
      try {
        const internals = window.__TAURI_INTERNALS__
        if (internals && typeof internals.invoke === 'function') {
          internals
            .invoke('player_bridge_report', { payload })
            .catch(() => {
              ipcBroken = true
              sendViaTitle(payload)
            })
          return
        }
      } catch (_) {
        /* fall through to the title-fallback channel */
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

  // --- Audio session keepalive -------------------------------------------
  // A permanently-open silent stream keeps this app's audio session alive in
  // Windows, so the volume mixer / routing tools (Wave Link, OBS) list the
  // app from launch instead of only while a song plays.
  let keepalive = null
  function startKeepalive() {
    if (keepalive) {
      if (keepalive.state === 'suspended') {
        keepalive.resume().catch(() => {})
      }
      return
    }
    try {
      const context = new AudioContext()
      const oscillator = context.createOscillator()
      const gain = context.createGain()
      // Not exactly zero: Chromium suspends output streams that render pure
      // digital silence, which drops the audio session to inactive and hides
      // the app from mixers. -140dB is far below audibility.
      gain.gain.value = 1e-7
      oscillator.frequency.value = 40
      oscillator.connect(gain)
      gain.connect(context.destination)
      oscillator.start()
      keepalive = context
      if (context.state === 'suspended') {
        context.resume().catch(() => {})
      }
    } catch (_) {
      keepalive = null
    }
  }

  // --- Output device routing ----------------------------------------------
  // The desired sink is pushed from the app settings. It is applied to every
  // current and future media element (and the keepalive context) so the
  // player's audio can be pointed at e.g. a Wave Link virtual device.
  const routing = { desired: '', devices: [], lastError: '' }

  function applySinkTo(element) {
    if (!element || typeof element.setSinkId !== 'function') {
      return
    }
    const target = routing.desired || ''
    if ((element.sinkId || '') === target) {
      return
    }
    element.setSinkId(target).catch((error) => {
      routing.lastError = String((error && error.message) || error)
    })
  }

  function applySinkEverywhere() {
    document.querySelectorAll('audio, video').forEach(applySinkTo)
    if (keepalive && typeof keepalive.setSinkId === 'function') {
      const target = routing.desired || ''
      keepalive.setSinkId(target === '' ? '' : target).catch(() => {})
    }
  }

  async function refreshOutputDevices() {
    try {
      const devices = await navigator.mediaDevices.enumerateDevices()
      routing.devices = devices
        .filter((device) => device.kind === 'audiooutput')
        .map((device, index) => ({
          id: device.deviceId,
          label: device.label || 'Output device ' + (index + 1),
        }))
    } catch (_) {
      routing.devices = []
    }
  }

  // Initialization scripts run before the document exists, so the observer
  // must attach lazily (a top-level observe(null) would kill this whole IIFE).
  let observerAttached = false
  function attachSinkObserver() {
    if (observerAttached || !document.documentElement) {
      return
    }
    try {
      new MutationObserver(() => applySinkEverywhere()).observe(document.documentElement, {
        childList: true,
        subtree: true,
      })
      observerAttached = true
    } catch (_) {
      /* retry on next tick */
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
      return {
        kind: 'status',
        ready: false,
        outputDevices: routing.devices,
        currentSink: routing.desired || '',
        sinkError: routing.lastError || null,
      }
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
      outputDevices: routing.devices,
      currentSink: routing.desired || '',
      sinkError: routing.lastError || null,
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
        // Routing works regardless of MusicKit/sign-in state.
        if (op === 'setSink') {
          routing.desired = id || ''
          routing.lastError = ''
          applySinkEverywhere()
          return done(
            true,
            routing.desired ? 'Audio routed to the selected output device.' : 'Audio routed to the system default output.',
          )
        }

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

  let tick = 0
  setInterval(() => {
    tick += 1
    attachSinkObserver()
    startKeepalive()
    attachEvents()
    applySinkEverywhere()
    if (tick % 5 === 1) {
      void refreshOutputDevices()
    }
    report(snapshot())
  }, 2000)
})();
