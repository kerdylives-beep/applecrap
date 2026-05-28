# AppleCrap Alpha

AppleCrap Alpha is a portable Windows app for taking song requests from Twitch chat and handing them off to Apple Music. It is built for streamers who want a small request desk beside their stream setup without running a full bot dashboard in the browser.

> Alpha means usable, but still early. Expect sharp edges, especially around Apple Music automation.

## Download

- ⬇️ [Download AppleCrap Alpha for Windows](https://github.com/kerdylives-beep/applecrap/releases/latest/download/AppleCrap.Alpha.zip)
- 📦 Latest portable zip: `v0.2.0-alpha.1`
- 🪟 Unzip it, run `AppleCrap Alpha.exe`, and keep the `data` folder beside it.

## What It Does

- 🎵 Listens for Twitch chat song requests like `!request Human Nature Michael Jackson`
- 🔎 Looks up likely Apple Music matches automatically
- 🧾 Keeps a live queue of requested songs
- ✅ Lets you approve, remove, or manually review requests
- 🎧 Opens matched tracks in Apple Music
- 🕹️ Includes experimental queue/playback automation for Windows Apple Music
- 🧰 Exports diagnostics if something goes sideways
- 💾 Stores data in the portable folder when possible

## Who It Is For

AppleCrap is for streamers who:

- use Twitch chat
- play music through Apple Music on Windows
- want viewers to request songs without manually copying every title
- prefer a portable app over a traditional installer
- are okay with testing an alpha build

It is probably not for you yet if you need a polished, signed, one-click production app.

## Screenshots

Screenshots would help a lot here. Good ones to add:

- 🏠 the main queue screen with a few sample requests
- ⚙️ the bot/settings setup screen
- 🧪 the diagnostics or automation panel

If you send me screenshots, I can add them to this README and make the GitHub page feel much more welcoming.

## Requirements

- 🪟 Windows
- 🎶 Apple Music for Windows
- 🌐 Microsoft WebView2 Runtime
- 💬 A Twitch bot account
- 🔑 A Twitch OAuth token for that bot account

The app asks for:

- Twitch channel name
- bot username
- bot OAuth token
- request command, usually `!request`

## How To Use The Portable Build

1. Download `AppleCrap Alpha.zip` from a release.
2. Unzip it somewhere you can write files, such as `Documents` or a stream tools folder.
3. Run `AppleCrap Alpha.exe`.
4. Keep the `data` folder next to the app.
5. Enter your Twitch bot settings.
6. Connect the bot and test a request in chat.

Portable storage uses `./data` beside the executable. If that folder is not writable, the app falls back to Local AppData and tells you in the UI.

## Chat Commands

Default request command:

```text
!request song title artist
```

Examples:

```text
!request Human Nature Michael Jackson
!request Freefall Durand Bernarr
!request https://music.apple.com/us/album/freefall-feat-durand-bernarr/1490035834?i=1490036368
```

Remove your latest request:

```text
!remove
```

## Safety Notes

- 🔐 Your Twitch token is stored locally in the app data file.
- 🧼 Diagnostics exports redact OAuth tokens.
- 🚪 External track opening is limited to Apple Music links.
- 🧪 Apple Music UI automation is experimental and can fail depending on the Windows app state.
- 🛟 Streamer-safe mode keeps automation behind explicit user actions unless you enable more.

## Building From Source

Most users do not need this section. It is here for developers and testers.

Install dependencies:

```bash
npm install
```

Run the frontend:

```bash
npm run dev
```

Run checks:

```bash
npm run lint
npm test
npm run build
```

Run the Tauri app in development:

```bash
npm run tauri:dev
```

Build the portable package:

```bash
npm run tauri:portable
```

The portable output is created at:

```text
release/portable/AppleCrap Alpha.zip
```

## Project Layout

- `src/` - React app, UI, typed Tauri bridge, and client state
- `src-tauri/` - Rust app shell, persistence, Twitch IRC, Apple Music lookup, diagnostics, and automation services
- `electron/` - legacy reference implementation kept for migration context
- `scripts/` - icon and portable packaging helpers

## Status

AppleCrap Alpha is early software. The core queue workflow is the priority:

```text
Twitch request -> Apple Music match -> streamer approval -> Apple Music handoff -> playback confirmation
```

Bug reports, screenshots, and real streamer workflow notes are very welcome.
