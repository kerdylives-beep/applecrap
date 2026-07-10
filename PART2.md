# Part 2 punch list — cleanup after the web-player bridge

Part 1 (commit `8698f07`) replaced the UIA/PowerShell automation with an
embedded music.apple.com player window driven through MusicKit
(`src-tauri/src/services/player_bridge.rs` + `src-tauri/src/scripts/player-bridge.js`).
The old machinery still compiles but is dead weight. Part 2 is mechanical
removal and polish — no architectural decisions left.

## 1. Delete the legacy automation stack

- `src-tauri/src/scripts/apple-music-automation.ps1` and `now-playing-probe.ps1`
- `src-tauri/src/services/automation_bridge.rs` and its `mod.rs` entry
- In `now_playing_probe.rs`: keep `snapshot_from_session`, `build_snapshot`,
  `match_queue_item` (the bridge uses them — consider moving them into
  `player_bridge.rs` and deleting the file). Delete `NowPlayingProbe` struct
  and the PowerShell plumbing.
- `settings_store.rs`: remove `write_support_scripts()` and `scripts_dir`
  (they exist only to copy the .ps1 files into the data dir).
- `commands.rs`/`lib.rs`: remove `run_automation_step` command.
- `models.rs`: remove `RunAutomationPayload`, `AutomationAction`,
  `AutomationAdapterKind`, `AutomationCapabilities`, `AutomationSnapshot`,
  and the `automation` field on `AppState`. In `AutomationSettings` drop
  `adapter`, `experimental_automation_enabled`, and `control_mode` (and the
  streamer-safe gating in `app.rs` `open_track`/`run_automation`); keep
  `handoff_mode`, `auto_arm_enabled`, `dispatch_hotkey`.
- `build.rs` + `capabilities/default.json`: remove `run_automation_step`
  entries.
- Frontend: delete the Automation panel and adapter/control-mode UI from
  `src/App.tsx`, prune `src/types.ts` and `src/useAppStore.ts` to match.
- Keep `cargo test` + `npm run build` green after each removal.

## 2. Player status strip (dashboard)

Surface bridge state on the dashboard: Connected / Loading / Sign-in
required (from the probe snapshot's `source: "apple-music-web"` states),
now-playing line, and the Show Player button (exists in the title bar as
"Player ↗"). Consider auto-showing the player window on first run when the
probe reports `SignInRequired`.

## 3. Chat commands (cheap wins)

- `!song` — reply with the probe's current title/artist.
- `!queue` — reply with requester's position and the next few titles.
- `!skip` (mods/broadcaster only) — `player_bridge.run_command(handle, "skip", None)`.
All wiring lives in `twitch_service.rs::handle_irc_line`.

## 4. Remove the debug badge

`player-bridge.js` renders a bottom-left channel/status badge for debugging.
Remove it (or gate it behind a setting) once stable.

## 5. README rewrite

Requirements change: Apple Music for Windows app NO LONGER required — just
WebView2 and an Apple Music subscription signed in inside the app's Player
window. Remove all UIA/automation/experimental language and the "flicker"
caveats. Document: first run → open Player → sign in → done.

## 6. Matcher refinement (optional)

"Human Nature Michael Jackson" now correctly avoids karaoke (tests exist),
but among *legitimate* same-song variants (movie/soundtrack re-releases)
the pick follows iTunes popularity order. If it bothers users: blend the
iTunes result index into `score_track` as a small prior, and/or penalize
soundtrack albums contextually.

## 7. Release verification

`npm run tauri:portable`, then on the unpacked build verify:
- player window works and stays signed in across restarts (WebView2 profile
  persists in the app data dir)
- queued track plays with **no user gesture** after a fresh boot
  (autoplay flag `--autoplay-policy=no-user-gesture-required` is set via
  `additionalBrowserArgs` on the main window in `tauri.conf.json`)
- audio keeps playing while the player window is hidden
- ACL works in release (capabilities/, build.rs app manifest)

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
