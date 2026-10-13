// Back and Forward for the embedded Apple Music player window, which has no
// browser toolbar. A small pill in the sidebar's top row, beside the Apple
// Music logo, plus the usual shortcuts (Alt+Left / Alt+Right) and the mouse's
// side buttons. The top-right corner looked free but isn't: playlist pages put
// their share and menu buttons there, and Search puts its tabs there.
//
// There is deliberately no Reload: reloading music.apple.com stops the music.
// Back and Forward stay inside Apple's single-page app, so playback goes on.
(function () {
  'use strict'
  if (window.top !== window || window.__applecrapNav) return
  window.__applecrapNav = true

  const nav = window.navigation
  const canGo = (step) => {
    if (nav && typeof nav.canGoBack === 'boolean') {
      return step < 0 ? nav.canGoBack : nav.canGoForward
    }
    return true
  }
  const go = (step) => {
    if (!canGo(step)) return
    if (step < 0) history.back()
    else history.forward()
  }

  // Shortcuts: handled here, and the default prevented, so the browser
  // doesn't also act on them and move two steps.
  window.addEventListener(
    'keydown',
    (event) => {
      if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault()
        event.stopPropagation()
        go(event.key === 'ArrowLeft' ? -1 : 1)
      }
    },
    true,
  )
  // Mouse side buttons (4 = back, 5 = forward).
  const sideButton = (event) => event.button === 3 || event.button === 4
  window.addEventListener(
    'mousedown',
    (event) => {
      if (sideButton(event)) event.preventDefault()
    },
    true,
  )
  window.addEventListener(
    'mouseup',
    (event) => {
      if (!sideButton(event)) return
      event.preventDefault()
      event.stopPropagation()
      go(event.button === 3 ? -1 : 1)
    },
    true,
  )

  const ICONS = {
    back: '<path d="M10.5 3.5 6 8l4.5 4.5" />',
    forward: '<path d="M5.5 3.5 10 8l-4.5 4.5" />',
  }
  const BUTTONS = [
    { id: 'back', step: -1, label: 'Back', keys: 'Alt+Left Arrow' },
    { id: 'forward', step: 1, label: 'Forward', keys: 'Alt+Right Arrow' },
  ]

  function mount() {
    if (!document.body || document.getElementById('applecrap-nav')) return
    const host = document.createElement('div')
    host.id = 'applecrap-nav'
    // A shadow root keeps Apple's styles off the pill and the pill's off Apple.
    const root = host.attachShadow({ mode: 'closed' })
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .pill {
          position: fixed;
          /* Until place() finds the sidebar. */
          top: 12px;
          left: 64px;
          z-index: 2147483647;
          display: flex;
          gap: 2px;
          padding: 3px;
          border-radius: 999px;
          background: rgba(40, 40, 42, 0.72);
          border: 1px solid rgba(255, 255, 255, 0.1);
          box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
          backdrop-filter: blur(16px) saturate(1.4);
          -webkit-backdrop-filter: blur(16px) saturate(1.4);
          font: 500 12px -apple-system, 'Segoe UI', system-ui, sans-serif;
        }
        button {
          position: relative;
          display: grid;
          place-items: center;
          width: 30px;
          height: 30px;
          padding: 0;
          border: 0;
          border-radius: 999px;
          background: transparent;
          color: rgba(255, 255, 255, 0.9);
          cursor: pointer;
          transition: background-color 120ms ease, color 120ms ease;
        }
        button:hover:not(:disabled) { background: rgba(255, 255, 255, 0.14); }
        button:active:not(:disabled) { background: rgba(255, 255, 255, 0.22); }
        button:focus-visible { outline: 2px solid #fa2d48; outline-offset: 1px; }
        button:disabled { color: rgba(255, 255, 255, 0.3); cursor: default; }
        svg {
          width: 16px;
          height: 16px;
          fill: none;
          stroke: currentColor;
          stroke-width: 1.9;
          stroke-linecap: round;
          stroke-linejoin: round;
        }
        /* Shows straight away, unlike the native tooltip's delay. */
        .tip {
          position: absolute;
          top: calc(100% + 8px);
          left: 0;
          padding: 5px 9px;
          border-radius: 7px;
          background: rgba(20, 20, 22, 0.94);
          border: 1px solid rgba(255, 255, 255, 0.1);
          color: #fff;
          white-space: nowrap;
          pointer-events: none;
          opacity: 0;
          transform: translateY(-3px);
          transition: opacity 120ms ease, transform 120ms ease;
        }
        .tip span { color: rgba(255, 255, 255, 0.55); }
        button:hover .tip,
        button:focus-visible .tip { opacity: 1; transform: none; }
        @media (prefers-color-scheme: light) {
          .pill { background: rgba(250, 250, 252, 0.78); border-color: rgba(0, 0, 0, 0.1); box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12); }
          button { color: rgba(0, 0, 0, 0.82); }
          button:hover:not(:disabled) { background: rgba(0, 0, 0, 0.08); }
          button:active:not(:disabled) { background: rgba(0, 0, 0, 0.14); }
          button:disabled { color: rgba(0, 0, 0, 0.25); }
        }
        @media (prefers-reduced-motion: reduce) {
          button, .tip { transition: none; }
        }
      </style>
      <div class="pill" role="toolbar" aria-label="Navigation">
        ${BUTTONS.map(
          (button) => `
          <button type="button" data-step="${button.step}" aria-label="${button.label} (${button.keys})">
            <svg viewBox="0 0 16 16" aria-hidden="true">${ICONS[button.id]}</svg>
            <span class="tip" aria-hidden="true">${button.label} <span>(${button.keys})</span></span>
          </button>`,
        ).join('')}
      </div>`

    const buttons = Array.from(root.querySelectorAll('button'))
    buttons.forEach((button) => {
      button.addEventListener('click', () => go(Number(button.dataset.step)))
    })
    const refresh = () => {
      buttons.forEach((button) => {
        button.disabled = !canGo(Number(button.dataset.step))
      })
    }
    refresh()
    if (nav) nav.addEventListener('currententrychange', refresh)
    window.addEventListener('popstate', refresh)
    document.documentElement.appendChild(host)

    const pill = root.querySelector('.pill')
    const place = () => {
      const spot = findSpot(pill.offsetWidth, pill.offsetHeight)
      if (!spot) return
      pill.style.left = Math.round(spot.left) + 'px'
      pill.style.top = Math.round(spot.top) + 'px'
    }
    place()
    window.addEventListener('resize', place)
    if (nav) nav.addEventListener('currententrychange', () => setTimeout(place, 50))
    // Apple's app can rebuild its sidebar; a cheap check keeps the pill on it.
    setInterval(place, 2000)
  }

  // Where the pill goes, following Apple's own layout:
  // - a normal window has a sidebar: its top row, right-aligned, level with
  //   the logo;
  // - a narrow window folds the sidebar into a top bar (menu button left,
  //   logo middle, Sign In or the account on the right): just after the menu
  //   button.
  function findSpot(width, height) {
    const sidebar = document.querySelector('[data-testid="navigation"]')
    if (!sidebar) return null
    const bar = sidebar.getBoundingClientRect()
    if (bar.width === 0 || bar.height === 0) return null
    const logo = sidebar.querySelector('.logo a') || sidebar.querySelector('.logo')
    const logoBox = logo && logo.getBoundingClientRect()

    if (bar.height > 120 && logoBox && logoBox.height > 0) {
      const left = Math.max(logoBox.right + 12, bar.right - 14 - width)
      return { left, top: logoBox.top + logoBox.height / 2 - height / 2 }
    }
    const menu = sidebar.querySelector('button.menuicon')
    const menuBox = menu && menu.getBoundingClientRect()
    const left = menuBox && menuBox.width > 0 ? menuBox.right + 8 : bar.left + 12
    return { left, top: bar.top + bar.height / 2 - height / 2 }
  }

  // Init scripts run before the page exists; wait for it.
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', mount, { once: true })
  } else {
    mount()
  }
})()
