// A Filter box for playlists in the embedded Apple Music player, like the one
// in Apple's desktop app. The web player only loads a playlist's songs as you
// scroll, so the browser's own find (Ctrl+F) can't see most of them. This box
// asks Apple Music for the whole track list through the page's own MusicKit
// (the same signed-in session the page uses), filters it as you type, and
// plays a match from that point in the playlist or queues it next.
//
// It sits beside the Back/Forward pill (player-nav.js) and only shows on
// playlist pages. Ctrl+F jumps to it there.
(function () {
  'use strict'
  if (window.top !== window || window.__applecrapFilter) return
  window.__applecrapFilter = true

  const PAGE_SIZE = 100
  const CACHE_FOR_MS = 10 * 60 * 1000
  const MAX_SHOWN = 200
  const cache = new Map() // playlist id -> { tracks, total, done, at, loading }

  const musicKit = () => {
    try {
      return window.MusicKit && window.MusicKit.getInstance()
    } catch (_) {
      return null
    }
  }

  // "/library/playlist/p.abc" (your library) or "/us/playlist/name/pl.abc"
  // (Apple's and shared playlists).
  function currentPlaylist() {
    const parts = location.pathname.split('/').filter(Boolean)
    const at = parts.indexOf('playlist')
    if (at === -1) return null
    const id = parts[parts.length - 1]
    if (/^p\./.test(id) && parts.includes('library')) return { id, library: true }
    if (/^pl\./.test(id)) return { id, library: false }
    return null
  }

  const fold = (text) =>
    String(text || '')
      .normalize('NFD')
      .replace(/[̀-ͯ]/g, '')
      .toLowerCase()
  // Words without accents or punctuation; "T-Pain" also counts as "tpain".
  const words = (text) => {
    const raw = fold(text).replace(/['’]/g, '').split(/[^a-z0-9]+/).filter(Boolean)
    const joined = fold(text)
      .split(/\s+/)
      .map((chunk) => chunk.replace(/[^a-z0-9]/g, ''))
      .filter((chunk) => chunk && !raw.includes(chunk))
    return raw.concat(joined)
  }

  function toTrack(item, index) {
    const a = item.attributes || {}
    const play = a.playParams || {}
    return {
      index,
      id: String(play.id || item.id || ''),
      catalogId: String(play.catalogId || ''),
      title: a.name || 'Untitled',
      artist: a.artistName || '',
      album: a.albumName || '',
      ms: a.durationInMillis || 0,
      words: words([a.name, a.artistName, a.albumName].join(' ')),
      titleWords: words(a.name),
      titleKey: words(a.name).join(' '),
      artistKey: words(a.artistName).join(' '),
    }
  }

  // Loads every track, page by page, calling onProgress as pages arrive.
  async function load(playlist, onProgress) {
    const known = cache.get(playlist.id)
    if (known && (known.loading || Date.now() - known.at < CACHE_FOR_MS)) return known
    const music = musicKit()
    if (!music || !music.api || typeof music.api.music !== 'function') {
      throw new Error('Apple Music is still starting up.')
    }
    const entry = { tracks: [], total: 0, done: false, at: Date.now(), loading: true, error: '' }
    cache.set(playlist.id, entry)
    const path = playlist.library
      ? `v1/me/library/playlists/${playlist.id}/tracks`
      : `v1/catalog/${music.storefrontId || 'us'}/playlists/${playlist.id}/tracks`
    try {
      let offset = 0
      for (;;) {
        const response = await music.api.music(path, { limit: PAGE_SIZE, offset })
        const body = (response && response.data) || {}
        const items = body.data || []
        items.forEach((item) => entry.tracks.push(toTrack(item, entry.tracks.length)))
        entry.total = (body.meta && body.meta.total) || entry.tracks.length
        offset += items.length
        onProgress(entry)
        if (!body.next || items.length === 0) break
      }
      entry.done = true
    } catch (error) {
      entry.error = (error && (error.message || error.errorCode)) || 'Couldn’t load the songs.'
      cache.delete(playlist.id)
    } finally {
      entry.loading = false
      onProgress(entry)
    }
    return entry
  }

  // Every typed word must start a word in the title, artist or album ("cha"
  // finds "Cha Cha Cha" but not "Michaels"). Best first: titles that begin
  // with what was typed, then titles containing it, then whole-word matches
  // in the title ("Cha Cha") before partial ones ("Chardonnay"), then
  // matches in the artist or album; ties keep playlist order.
  function search(tracks, query) {
    const typed = words(query)
    if (!typed.length) return []
    const phrase = typed.join(' ')
    const startsAWord = (list, word) => list.some((candidate) => candidate.startsWith(word))
    const hits = tracks.filter((track) => typed.every((word) => startsAWord(track.words, word)))
    const score = (track) => {
      let points = 0
      if (track.titleKey.startsWith(phrase)) points += 1000
      // Typing an artist's name lists their own songs before features.
      else if (track.artistKey.startsWith(phrase)) points += 600
      else if ((' ' + track.titleKey).includes(' ' + phrase)) points += 500
      for (const word of typed) {
        if (track.titleWords.includes(word)) points += 20
        else if (startsAWord(track.titleWords, word)) points += 10
        else if (track.words.includes(word)) points += 5
        else points += 2
      }
      return points
    }
    const scored = hits.map((track) => ({ track, points: score(track) }))
    scored.sort((left, right) => right.points - left.points || left.track.index - right.track.index)
    return scored.map((entry) => entry.track)
  }

  const idOf = (item) => {
    const params = (item && item.attributes && item.attributes.playParams) || (item && item.playParams) || {}
    return String(params.catalogId || params.id || (item && item.id) || '')
  }
  // Every id a queue item can be known by (library "i.…", catalog number).
  const idsOf = (item) => {
    const params = (item && item.attributes && item.attributes.playParams) || (item && item.playParams) || {}
    return [item && item.id, params.id, params.catalogId].filter(Boolean).map(String)
  }

  // Requests AppleCrap handed to the player that are still waiting to play.
  // Replacing the queue would drop them, so they're put back afterwards.
  function pendingRequests(music) {
    const queued = window.__applecrapQueued
    const queue = music.queue
    if (!queued || !queued.size || !queue || !queue.items) return []
    const position = typeof queue.position === 'number' ? queue.position : -1
    return queue.items
      .filter((item, index) => index > position)
      .map(idOf)
      .filter((id) => queued.has(id))
  }

  // A MusicKit call that never settles must not leave the box hanging.
  const within = (promise, ms, message) =>
    Promise.race([promise, new Promise((_, reject) => setTimeout(() => reject(new Error(message)), ms))])

  async function playFrom(playlist, track) {
    const music = musicKit()
    if (!music) throw new Error('Apple Music is still starting up.')
    const keep = pendingRequests(music)
    const want = [track.id, track.catalogId].filter(Boolean)
    const isWanted = (item) => !!item && idsOf(item).some((id) => want.includes(id))

    // Shuffle reorders the queue setQueue builds, so "start at song N" would
    // land on a random song. Load it in order, then shuffle the rest again,
    // the way Apple's desktop app plays a picked song first.
    const shuffle = music.shuffleMode
    if (shuffle) music.shuffleMode = 0
    try {
      await within(
        music.setQueue({ playlist: playlist.id, startWith: track.index }),
        20000,
        'Apple Music took too long to load the playlist.',
      )
      await within(music.play(), 15000, 'Apple Music didn’t start playing.')
      // Songs that can't play are left out of the queue, which shifts the
      // positions; if it isn't the picked song, find the song itself.
      if (!isWanted(music.queue && music.queue.currentItem)) {
        const items = (music.queue && music.queue.items) || []
        const at = items.findIndex(isWanted)
        if (at >= 0 && typeof music.changeToMediaAtIndex === 'function') {
          await within(music.changeToMediaAtIndex(at), 15000, 'Apple Music didn’t switch to the song.')
        }
      }
    } finally {
      if (shuffle) music.shuffleMode = shuffle
    }
    if (keep.length) {
      try {
        await music.playNext({ songs: keep })
      } catch (_) {
        for (const id of keep.reverse()) await music.playNext({ song: id })
      }
    }
  }

  async function playNext(track) {
    const music = musicKit()
    if (!music) throw new Error('Apple Music is still starting up.')
    await within(music.playNext({ song: track.catalogId || track.id }), 15000, 'Apple Music didn’t respond.')
  }

  const time = (ms) => {
    const seconds = Math.round(ms / 1000)
    return Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0')
  }
  const escape = (text) =>
    String(text).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])

  function mount() {
    if (!document.body || document.getElementById('applecrap-filter')) return
    const host = document.createElement('div')
    host.id = 'applecrap-filter'
    const root = host.attachShadow({ mode: 'closed' })
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .box {
          position: fixed;
          top: 6px;
          left: 340px;
          z-index: 2147483646;
          font: 500 13px -apple-system, 'Segoe UI', system-ui, sans-serif;
          color: rgba(255, 255, 255, 0.92);
        }
        .box[hidden] { display: none; }
        .field {
          display: flex;
          align-items: center;
          gap: 6px;
          width: 220px;
          height: 30px;
          box-sizing: border-box;
          padding: 0 10px;
          border-radius: 999px;
          background: rgba(40, 40, 42, 0.72);
          border: 1px solid rgba(255, 255, 255, 0.1);
          box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
          backdrop-filter: blur(16px) saturate(1.4);
          -webkit-backdrop-filter: blur(16px) saturate(1.4);
          transition: width 150ms ease, border-color 150ms ease;
        }
        .field:focus-within { width: 300px; border-color: rgba(250, 45, 72, 0.8); }
        svg { flex: none; width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; opacity: 0.7; }
        input {
          flex: 1;
          min-width: 0;
          border: 0;
          outline: 0;
          background: transparent;
          color: inherit;
          font: inherit;
        }
        input::placeholder { color: rgba(255, 255, 255, 0.5); }
        .panel {
          position: absolute;
          top: 38px;
          left: 0;
          width: 420px;
          max-height: min(60vh, 520px);
          display: flex;
          flex-direction: column;
          border-radius: 12px;
          background: rgba(28, 28, 30, 0.96);
          border: 1px solid rgba(255, 255, 255, 0.1);
          box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
          overflow: hidden;
        }
        .panel[hidden] { display: none; }
        .status { padding: 8px 12px; font-size: 12px; color: rgba(255, 255, 255, 0.55); border-bottom: 1px solid rgba(255, 255, 255, 0.07); }
        .list { overflow-y: auto; overflow-x: hidden; }
        .row {
          display: grid;
          /* minmax(0, …) lets long names cut off instead of widening the row. */
          grid-template-columns: minmax(0, 1fr) auto auto;
          align-items: center;
          gap: 10px;
          padding: 7px 8px 7px 12px;
          cursor: pointer;
        }
        .row.active, .row:hover { background: rgba(255, 255, 255, 0.08); }
        .name, .meta { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
        .meta { font-size: 12px; color: rgba(255, 255, 255, 0.55); }
        .time { font-size: 12px; color: rgba(255, 255, 255, 0.5); font-variant-numeric: tabular-nums; }
        .next {
          width: 26px;
          height: 26px;
          display: grid;
          place-items: center;
          border: 0;
          border-radius: 999px;
          background: transparent;
          color: inherit;
          cursor: pointer;
          opacity: 0;
        }
        .row:hover .next, .row.active .next { opacity: 1; }
        .next:hover { background: rgba(255, 255, 255, 0.14); }
        .next svg { opacity: 0.9; }
        @media (prefers-color-scheme: light) {
          .box { color: rgba(0, 0, 0, 0.85); }
          .field { background: rgba(250, 250, 252, 0.85); border-color: rgba(0, 0, 0, 0.1); box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12); }
          input::placeholder { color: rgba(0, 0, 0, 0.45); }
          .panel { background: rgba(252, 252, 253, 0.98); border-color: rgba(0, 0, 0, 0.1); box-shadow: 0 12px 32px rgba(0, 0, 0, 0.18); }
          .status { color: rgba(0, 0, 0, 0.5); border-color: rgba(0, 0, 0, 0.07); }
          .row.active, .row:hover { background: rgba(0, 0, 0, 0.06); }
          .meta, .time { color: rgba(0, 0, 0, 0.5); }
          .next:hover { background: rgba(0, 0, 0, 0.08); }
        }
        @media (prefers-reduced-motion: reduce) { .field { transition: none; } }
      </style>
      <div class="box" hidden>
        <label class="field">
          <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
          <input type="search" placeholder="Filter (Ctrl+F)" aria-label="Filter this playlist" role="combobox"
                 aria-expanded="false" aria-controls="results" autocomplete="off" spellcheck="false" />
        </label>
        <div class="panel" hidden>
          <div class="status" aria-live="polite"></div>
          <div class="list" id="results" role="listbox" aria-label="Matching songs"></div>
        </div>
      </div>`
    document.documentElement.appendChild(host)

    const box = root.querySelector('.box')
    const input = root.querySelector('input')
    const panel = root.querySelector('.panel')
    const status = root.querySelector('.status')
    const list = root.querySelector('.list')
    let playlist = null
    let matches = []
    let active = 0

    const place = () => {
      const nav = window.__applecrapNavBox
      if (!nav) return
      box.style.left = Math.round(nav.right + 8) + 'px'
      box.style.top = Math.round(nav.top + nav.height / 2 - 15) + 'px'
    }

    const showPanel = (open) => {
      panel.hidden = !open
      input.setAttribute('aria-expanded', String(open))
    }

    function describe(entry) {
      if (entry.error) return `Couldn’t load the songs: ${entry.error}`
      const count = matches.length
      const found = !input.value.trim()
        ? `${entry.tracks.length.toLocaleString()} songs`
        : `${count.toLocaleString()} ${count === 1 ? 'match' : 'matches'}` +
          (count > MAX_SHOWN ? ` (first ${MAX_SHOWN} shown)` : '')
      return entry.done || !entry.loading
        ? found
        : `${found} · loading ${entry.tracks.length.toLocaleString()} of ${entry.total.toLocaleString()}…`
    }

    function render() {
      const entry = playlist && cache.get(playlist.id)
      if (!entry || !input.value.trim()) {
        matches = []
        list.innerHTML = ''
        status.textContent = entry ? describe(entry) : 'Loading songs…'
        return
      }
      matches = search(entry.tracks, input.value)
      active = Math.min(active, Math.max(0, matches.length - 1))
      status.textContent = describe(entry)
      list.innerHTML = matches
        .slice(0, MAX_SHOWN)
        .map(
          (track, i) => `
          <div class="row${i === active ? ' active' : ''}" role="option" aria-selected="${i === active}" data-i="${i}"
               title="Play from here">
            <div>
              <div class="name">${escape(track.title)}</div>
              <div class="meta">${escape([track.artist, track.album].filter(Boolean).join(' · '))}</div>
            </div>
            <span class="time">${track.ms ? time(track.ms) : ''}</span>
            <button class="next" type="button" data-next="${i}" aria-label="Play next" title="Play Next (Shift+Enter)">
              <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4h7M3 8h7M3 12h4M12 9v5M9.5 11.5h5" /></svg>
            </button>
          </div>`,
        )
        .join('')
    }

    async function ensureLoaded() {
      playlist = currentPlaylist()
      if (!playlist) return
      render()
      try {
        await load(playlist, () => render())
      } catch (error) {
        status.textContent = (error && error.message) || 'Couldn’t load the songs.'
      }
      render()
    }

    async function choose(i, next) {
      const track = matches[i]
      if (!track || !playlist) return
      try {
        if (next) {
          await playNext(track)
          status.textContent = `“${track.title}” will play next.`
        } else {
          status.textContent = `Starting “${track.title}”…`
          await playFrom(playlist, track)
          input.blur()
          showPanel(false)
        }
      } catch (error) {
        status.textContent = `Couldn’t play that: ${(error && error.message) || error}`
      }
    }

    input.addEventListener('focus', () => {
      showPanel(true)
      ensureLoaded()
    })
    input.addEventListener('input', () => {
      active = 0
      showPanel(true)
      render()
    })
    input.addEventListener('keydown', (event) => {
      // Keep Apple's page shortcuts (like Space for play) out of the box.
      event.stopPropagation()
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const last = Math.min(matches.length, MAX_SHOWN) - 1
        if (last < 0) return
        active = event.key === 'ArrowDown' ? Math.min(last, active + 1) : Math.max(0, active - 1)
        render()
        const row = list.querySelector('.row.active')
        if (row) row.scrollIntoView({ block: 'nearest' })
      } else if (event.key === 'Enter') {
        event.preventDefault()
        choose(active, event.shiftKey)
      } else if (event.key === 'Escape') {
        event.preventDefault()
        if (input.value) {
          input.value = ''
          render()
        } else {
          input.blur()
          showPanel(false)
        }
      }
    })
    list.addEventListener('mousedown', (event) => event.preventDefault()) // keep focus in the box
    list.addEventListener('click', (event) => {
      const next = event.target.closest('[data-next]')
      if (next) return choose(Number(next.dataset.next), true)
      const row = event.target.closest('.row')
      if (row) choose(Number(row.dataset.i), false)
    })
    input.addEventListener('blur', () => setTimeout(() => {
      if (root.activeElement !== input) showPanel(false)
    }, 150))

    // Ctrl+F on a playlist page: the browser's find can't see unloaded songs.
    window.addEventListener(
      'keydown',
      (event) => {
        if (!box.hidden && (event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'f') {
          event.preventDefault()
          event.stopPropagation()
          input.focus()
          input.select()
        }
      },
      true,
    )

    // Show only on playlist pages; Apple's site changes pages without loading.
    let lastPath = ''
    const sync = () => {
      if (location.pathname === lastPath) return
      lastPath = location.pathname
      const onPlaylist = !!currentPlaylist()
      box.hidden = !onPlaylist
      input.value = ''
      matches = []
      showPanel(false)
      playlist = null
      if (onPlaylist) place()
    }
    sync()
    window.addEventListener('applecrap-nav-placed', place)
    if (window.navigation) window.navigation.addEventListener('currententrychange', () => setTimeout(sync, 50))
    window.addEventListener('popstate', sync)
    setInterval(sync, 1000)
    place()
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', mount, { once: true })
  } else {
    mount()
  }
})()
