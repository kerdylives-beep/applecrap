# AppleCrap

AppleCrap is a portable Windows app for taking song requests from Twitch chat and Channel Points and playing them through Apple Music. It is built for streamers who want a small request desk beside their stream setup without running a full bot dashboard in the browser.

> Beta: it works and it's used on real streams, but you may still find rough edges.

## Download

- ⬇️ [Download AppleCrap for Windows](https://github.com/kerdylives-beep/applecrap/releases/latest/download/AppleCrap.zip)
- 📦 [All releases and what changed](https://github.com/kerdylives-beep/applecrap/releases)
- 🪟 Unzip it and run `AppleCrap.exe`. Your settings live in the `data` folder next to it.

**"Windows protected your PC"?** The first time you run it, Windows may show this because the app
isn't code-signed yet (signing costs money every month). Click **More info**, then **Run anyway**.
It only asks once, and updates never trigger it again.

## What It Does

- 🎵 Listens for Twitch chat song requests like `!request Human Nature Michael Jackson`
- 💎 Optionally takes requests through a Channel Points reward, refunding points when a request can't be queued
- 🔎 Looks up likely Apple Music matches automatically
- 🧾 Keeps a live queue of requested songs
- ✅ Lets you approve, remove, or pick the right match for a request, right in the queue
- 🎧 Queues matched tracks straight into Apple Music as Play Next, so they play automatically and the streamer's playlist resumes once requests run out
- 📺 Serves a now-playing overlay for OBS (current song, requester, and what is next)
- 🛟 Tells you plainly when something needs attention (a crash, Apple Music trouble, chat offline) and saves a problem report in one click
- ♻️ Updates itself, and switches back to the previous version automatically if an update won't start
- 💾 Stores data in the portable folder when possible

## Who It Is For

AppleCrap is for streamers who:

- use Twitch chat
- play music through Apple Music
- want viewers to request songs without manually copying every title
- prefer a portable app over a traditional installer
- are okay with a beta

## Screenshots

The request desk: what's playing and who asked for it, with the queue underneath. Anything that needs you (like a request with no match yet) stands out:

<img src="img/screenshots/desk.png" width="420" alt="The AppleCrap desk: Human Nature by Michael Jackson playing, requested with Channel Points, and four requests queued, one of them highlighted for review">

Give it a wider window and the activity feed joins in:

<img src="img/screenshots/desk-wide.png" width="720" alt="The AppleCrap desk in a wide window, with now playing on the left, the queue in the middle and recent activity on the right">

The embedded Apple Music player — sign in once, then it can stay hidden all stream while requests queue into it:

<img src="img/screenshots/player.png" width="720" alt="The embedded Apple Music player window, signed in and showing the Home page">

## Requirements

- 🪟 Windows
- 🌐 Microsoft WebView2 Runtime
- 🎶 An Apple Music subscription (you sign in once, inside the app's own Player window)
- 💬 A Twitch account (your channel's, or a separate bot account)
- 💎 For Channel Points requests: a Twitch Affiliate or Partner channel

The Apple Music for Windows desktop app is **not** required and is not used — AppleCrap plays music through its own embedded Apple Music web player.

## How To Use

1. Download `AppleCrap.zip` from a release, unzip it somewhere you can write files, and run `AppleCrap.exe`.
2. A short setup guide walks you through the rest:
   - **Sign in to Apple Music** in the app's own player window. You only do this once.
   - **Sign in with Twitch**: enter the code Twitch shows you at twitch.tv/activate. No token copying — AppleCrap keeps the sign-in fresh on its own. You can add a separate bot account later in **Setup** if you want replies to come from a bot.
   - **Connect to chat.**
3. That's it. Matched requests auto-queue into Apple Music in order (FIFO), each one is confirmed once it actually starts playing, and the streamer's own playlist picks back up automatically once the request queue is empty.

Portable storage uses `./data` beside the executable. If that folder is not writable, the app falls back to Local AppData and tells you in the UI.

Your keyboard's **media keys** (play/pause, next, previous) control the player while it has a track loaded — no need to focus the app first. When AppleCrap is not holding a track, the keys go back to your other apps, so they only take over the keys while they are actually the thing playing. You can turn this off in **Setup**. The current track also shows in the Windows "now playing" popup.

The player window can stay hidden the whole time; while hidden it stops drawing entirely, so it costs almost nothing to leave running beside a game.

**Audio quality:** playback runs at 256 kbps AAC, the highest the Apple Music web player offers, and
AppleCrap pins it there so it can never drop to the lower setting. Lossless and Spatial Audio are
exclusive to Apple's own native apps and are not available to any browser-based player. For a stream
this makes no practical difference — Twitch re-encodes all audio to 160 kbps AAC or lower before it
reaches viewers. The current bitrate is shown under the now-playing card.

## Stream Overlay

AppleCrap serves a now-playing overlay for OBS on your own machine.

1. Open the **Overlay** panel in the app and copy the URL (`http://127.0.0.1:4747/` by default).
2. In OBS, add a **Browser** source and paste it in. A size of about **560 x 220** fits the card.
3. That is it — the background is transparent, so it sits straight over your scene.

It shows the current song with its artwork, who requested it, and what is queued up next. It
fades itself out when nothing is playing, so an idle scene stays clean. The panel can turn the
"next up" list off, change how many upcoming songs it lists, or move it to another port.

## Channel Points Requests

Affiliate and Partner channels can take requests through Channel Points instead of (or as well as) the chat command.

1. Sign in as your channel (see above).
2. In **Setup**, tick **Take song requests through Channel Points** and pick a reward name and cost. It saves by itself.

AppleCrap creates the reward on your channel. Each redemption goes through the same queue rules as
`!request`: if the song is queued, the redemption is marked complete; if it is turned away (queue
full, duplicate, too long), the viewer's points are refunded and they are told why in chat.
Redemptions made while the app was closed are picked up the next time it starts (ones older than
six hours are refunded instead). Turning the feature off hides the reward.

Tick **Only take requests through Channel Points** to have `!request` point viewers at the reward
instead. Mods can still use the chat command.

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

Check what's currently playing:

```text
!song
```

Check your position in the queue and a preview of what's up next:

```text
!queue
```

Skip the current track (mods/broadcaster only):

```text
!skip
```

## Privacy

AppleCrap has no servers and no analytics. Everything stays on your PC except what it has to send:

- **Twitch** — to read and reply in your chat, and (if you use it) to manage your Channel Points reward.
- **Apple** — song searches, and the Apple Music player itself.
- **GitHub** — to check for and download updates.

A problem report is a file saved on your PC; nothing is sent unless you attach it to an email yourself.
It contains your settings, queue and recent activity, never passwords or sign-in tokens.

## Safety Notes

- 🔐 Twitch sign-ins are stored in the app data file, encrypted with your Windows account (DPAPI), so a copied `state.json` is useless on another PC or user.
- 🧼 Diagnostics exports never include tokens.
- ✍️ Updates are signed; the app refuses to install an update whose signature doesn't check out.
- 🏠 The overlay only answers requests from your own machine.
- ↩️ If an update won't start, AppleCrap puts the previous version back and won't try that update again.
- 🚪 Track links are limited to Apple Music.
- 🎚️ Auto-queue can be paused from the dashboard; a "Send now" action is always available for the front request when you want manual control.

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

Release builds are signed with an Ed25519 key read from `APPLECRAP_SIGNING_KEY` or
`~/.applecrap/release-signing-key.pem`; upload the `.zip.sig` beside the zip, or the app won't
auto-install the update. The packaging step then checks the zip exactly as the updater will
(`npm run verify:release` runs the same check by hand). Twitch sign-in needs the app's Twitch client ID (a public value), built in
from `src-tauri/src/services/twitch_auth.rs` or the `APPLECRAP_TWITCH_CLIENT_ID` environment variable.

The portable output is created at:

```text
release/portable/AppleCrap.zip
```

To work on the UI without the app, run `npm run dev` and open http://127.0.0.1:1420 in a browser: a
demo backend fills it with sample data (`?demo=empty`, `?demo=firstrun` and `?demo=alerts` show
other states).

## Project Layout

- `src/` - React app: `components/` for the screens, `useAppStore.ts` for client state, `tauri.ts` for the typed bridge to the backend
- `src-tauri/` - Rust app shell, persistence, Twitch chat, sign-in and Channel Points, Apple Music lookup, the embedded player bridge, and diagnostics
- `scripts/` - icon and portable packaging helpers

## Status

AppleCrap is in beta. The core queue workflow is the priority:

```text
Twitch request or redemption -> Apple Music match -> auto-queue (Play Next) -> playback confirmation
```

Bug reports, screenshots, and real streamer workflow notes are very welcome.
