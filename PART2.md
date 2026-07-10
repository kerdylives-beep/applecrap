# Part 2 punch list — cleanup after the web-player bridge

Part 1 (commit `8698f07`) replaced the UIA/PowerShell automation with an
embedded music.apple.com player window driven through MusicKit
(`src-tauri/src/services/player_bridge.rs` + `src-tauri/src/scripts/player-bridge.js`).
Part 2 executed the mechanical removal/polish below.

## 1. Delete the legacy automation stack — DONE

Removed the `.ps1` scripts, `automation_bridge.rs`, and the standalone
`now_playing_probe.rs` module (its snapshot/matching logic and tests moved
into `player_bridge.rs`). Removed `write_support_scripts()`/`scripts_dir`,
the `run_automation_step` command, and the automation-adapter model types
(`AutomationAdapterKind`, `AutomationAction`, `AutomationCapabilities`,
`AutomationRunResult`, `AutomationSnapshot`).

## 2. Simplify the control model — DONE (superseded the original plan)

Rather than keeping `AutomationControlMode`/`AutomationHandoffMode`/the
dispatch hotkey around, the control model was collapsed further: Play
Next (`queueNext`) is now the only dispatch behaviour, the streamer-safe
gating is gone, and `AutomationSettings` was replaced by a single
`PlayerSettings { auto_queue: bool }` (aliased from the old `automation`
JSON key so existing `state.json` files still load). The dispatch hotkey
feature (setting, command, `tauri-plugin-global-shortcut` dependency) was
removed entirely. `dispatch_next_request` remains as a manual "send now"
action for the front request.

## 3. Player status strip (dashboard) — DONE

The dashboard surfaces Connected / Loading / Sign-in required /
Disconnected (derived from the probe snapshot), the current now-playing
line, and a "Show player" button that becomes "Open player to sign in"
with a warn tone when sign-in is required.

## 4. Chat commands (cheap wins) — DONE

- `!song` — replies with the probe's current title/artist (`format_song_reply`
  in `twitch_service.rs`), sourced from a new read-only `AppContext::current_probe()`
  accessor.
- `!queue` — replies with the requester's position(s) and a preview of the
  next few titles (`queue_engine::format_queue_reply`, unit tested), sourced
  from a new read-only `AppContext::current_queue()` accessor.
- `!skip` (mods/broadcaster only) — calls
  `player_bridge.run_command(&context.handle, "skip", None)`; non-mods get
  "Only mods can skip."
All wiring lives in `twitch_service.rs::handle_irc_line`. The configured
request command is checked first so it can never be shadowed by these
built-in names (e.g. setting `!song` as the request command still requests).

## 5. Remove the debug badge — DONE

Removed the bottom-left channel/status badge from `player-bridge.js`; the
IPC-with-title-fallback channel logic is unchanged.

## 6. README rewrite — DONE

Requirements now list just Windows, WebView2, and an Apple Music
subscription signed in inside the app's Player window. Removed all
UIA/automation/experimental/flicker language.

## 9. Visual rework backlog (planned next)

The app is maintained for a friend who streams — the target user is NOT the
developer, so first-run self-service matters most. When redoing the visuals:

- **Twitch OAuth token onboarding is the roughest edge.** The Bot settings
  just ask for a token "starting with oauth:"; a non-technical user has no
  idea where to get one. Add an in-app walkthrough (link or short steps
  next to the token field) and a matching "Getting your bot token" section
  in the README.
- Consider auto-showing the player window on first run when the probe
  reports SignInRequired, so sign-in is impossible to miss.
- After the redesign, re-run the screenshot pipeline (seed-queue.py +
  capture-window.ps1 with PrintWindow + name-censor pass — scripts were in
  the 2026-07-10 session scratchpad; they are small, recreate if gone) and
  overwrite `img/screenshots/*.png`; the README references fixed filenames
  so no README edits are needed. Sign the player into Browse/New first so
  no account name appears (avoids the censor pass entirely).

## 7. Matcher refinement — NOT DONE (still open, optional)

"Human Nature Michael Jackson" now correctly avoids karaoke (tests exist),
but among *legitimate* same-song variants (movie/soundtrack re-releases)
the pick follows iTunes popularity order. If it bothers users: blend the
iTunes result index into `score_track` as a small prior, and/or penalize
soundtrack albums contextually.

## 8. Release verification — NOT DONE (manual QA, do before shipping)

`npm run tauri:portable`, then on the unpacked build verify:
- player window works and stays signed in across restarts (WebView2 profile
  persists in the app data dir)
- queued track plays with **no user gesture** after a fresh boot
  (autoplay flag `--autoplay-policy=no-user-gesture-required` is set via
  `additionalBrowserArgs` on the main window in `tauri.conf.json`)
- audio keeps playing while the player window is hidden
- ACL works in release (capabilities/, build.rs app manifest)
- window fits on a 1080p display at both the default size and the
  560x700 minimum (`tauri.conf.json` window bounds)

## Known gotchas (learned in Part 1, do not re-derive)

- Adding ANY capability file flips Tauri from allow-all to enforced ACL:
  every window needs a capability, and app commands must be declared in
  `build.rs` `AppManifest::commands` to be grantable. Remote origins
  (music.apple.com) additionally need the `remote.urls` block.
- The bridge init script runs in every iframe; it bails unless
  `window.top === window.self`.
- `nowPlayingItem.id` can be a library ID (e.g. `XMDNONVFvqLO7x`), not a
  catalog ID — confirmation reads `playParams.catalogId` and falls back to
  fuzzy title/artist matching.
- The window-title fallback channel (`ACB1|base64`) exists in the bridge
  but has NOT been proven to propagate document.title → native title; IPC
  is the working channel. Don't rely on the fallback without testing it.
