# AppleCrap

AppleCrap is a Windows desktop app for streamers who want Twitch song requests for Apple Music without pretending the Apple Music Windows app has a clean public automation API.

In plain English, AppleCrap does four jobs:

1. It listens to Twitch chat for song requests.
2. It moderates those requests with queue rules and per-user limits.
3. It translates messy viewer input into real Apple Music tracks whenever possible.
4. It gives the streamer a clean handoff workflow: open the next song in Apple Music, then automatically clear it once playback is detected.

It is intentionally a **handoff desk**, not a full native Apple Music remote control. That distinction matters, because the entire product is built around what Apple Music on Windows actually allows today.

---

## What This App Is

AppleCrap is:

- a Windows Electron app
- a Twitch chat bot
- a local moderation console
- an Apple Music request resolver
- a playback-aware queue handoff tool

AppleCrap is not:

- a browser widget
- a cloud service
- a MusicKit-authenticated Apple playback client
- a true Apple Music queue injector
- a general-purpose streamer chatbot

The app exists because streamers still want Apple Music request workflows even though Apple does not offer the same kind of streamer-friendly queue automation that people expect from Spotify-based tools.

---

## The Core Product Idea

The product is built around one practical truth:

> Twitch viewers can request songs in plain text, but Apple Music on Windows does not expose a nice public “add this exact song to the queue right now” API for third-party desktop apps.

So AppleCrap solves the problem in layers:

- it captures requests from chat
- it resolves them into Apple Music tracks using Apple’s public search/lookup endpoints
- it presents the next request clearly in the UI
- it opens the exact track in Apple Music for the streamer
- it watches Windows playback state and removes the top request once the song is actually playing

That makes it feel closer to a real request bot even though the final “queue this in Apple Music” action is still a human handoff.

---

## Target User

The ideal AppleCrap user is:

- a Twitch streamer
- on Windows
- using the Apple Music desktop app
- comfortable running a desktop helper tool while live
- willing to approve or handoff songs manually

It is especially useful for:

- creators who strongly prefer Apple Music over Spotify
- streamers who want queue moderation rules
- people who want their request queue to clear itself when playback starts

---

## High-Level Feature Set

AppleCrap currently includes:

- a custom Electron desktop shell for Windows
- a React-based streamer UI
- Twitch chat integration via `tmi.js`
- persistent local settings and queue state
- a setup wizard for connecting a bot account
- queue moderation rules
- Apple Music text search matching
- direct Apple Music track-link matching
- manual testing tools
- activity logging
- Windows now-playing detection for Apple Music, Spotify, and Media Player
- auto-removal of the top request when the app detects that it has started playing
- Windows packaging via `electron-builder`

---

## The Streamer Workflow

The intended everyday flow looks like this:

1. The streamer launches AppleCrap.
2. They connect their Twitch bot account through the setup wizard.
3. Viewers use a request command in chat such as `!request`.
4. AppleCrap evaluates each message against moderation rules.
5. It tries to resolve the request to a real Apple Music track.
6. The top request appears in the large featured handoff area.
7. The streamer clicks `Open In Apple Music`.
8. The streamer starts playback in Apple Music.
9. AppleCrap detects that the matching song is playing.
10. AppleCrap removes the top request from the queue automatically.

That loop is the heart of the product.

---

## User Interface Overview

The UI is designed like a compact desktop control surface rather than a generic web dashboard.

### 1. Custom App Frame

The top frame bar contains:

- the AppleCrap icon
- the AppleCrap name
- menu-like buttons for:
  - `Bot`
  - `Rules`
  - `Testing`
  - `Now Playing`
  - `Log`
  - `About`

This frame is custom-styled so the app feels more like a native desktop tool and less like a web page inside a window.

### 2. Header / Identity Area

The top content header shows:

- a small subtitle: `KerdyLives`
- a large product title: `AppleCrap`
- the primary `Setup Bot` / `Bot Ready` / `Bot Connected` action
- a `Clear Queue` button

### 3. Queue Screen

The main queue area is split into:

- a section bar greeting the active channel: `Whatup, <channel>`
- a featured “On Deck” handoff panel for the next song
- a compact requests list underneath for everything behind the current song

### 4. Status Dock

The bottom dock shows live status:

- bot connection status
- number of queued items
- now-playing status
- either playback confirmation or bot detail text

This is pinned to the bottom of the window to keep the top of the UI focused on action rather than diagnostics.

---

## Setup Wizard

The bot setup flow is intentionally wizard-based so the main screen stays focused on queue management.

### Step 1: Channel

The streamer sets:

- Twitch channel name
- request command, such as `!request`
- Apple Music storefront, usually `us`

### Step 2: Bot Login

The streamer sets:

- bot username
- bot OAuth token
- optional auto-connect on app launch

This step includes two helper actions for Twitch token setup:

- `Open InPrivate`
- `Copy Link`

The token generator link is:

- `https://twitchtokengenerator.com/`

The reason for the private-window flow is practical: many streamers are already logged into their main Twitch account in their normal browser session, but the token must usually be generated while signed into the bot account instead.

AppleCrap therefore tries to open the token generator in **Microsoft Edge InPrivate**. If that fails, it copies the link and shows a status message.

### Step 3: Request Rules

The streamer sets moderation rules such as:

- queue size
- per-user request limit
- cooldown
- maximum track length
- whether duplicates are allowed
- whether links are allowed
- whether mods can bypass limits

When the wizard finishes, AppleCrap saves the settings and tries to connect the bot immediately.

---

## Twitch Bot Behavior

The bot is powered by `tmi.js` and joins the configured Twitch channel using the provided bot username and OAuth token.

### Supported Commands

AppleCrap currently supports:

- the configured request command, usually `!request`
- `!remove`

### `!request`

When a viewer sends a message such as:

```text
!request human nature michael jackson
```

or

```text
!request https://music.apple.com/us/album/freefall-feat-durand-bernarr/1490035834?i=1490036368
```

the app:

- strips the command prefix
- treats the remainder as the request payload
- checks moderation rules
- attempts track resolution
- appends the accepted request to the back of the queue
- replies in Twitch chat with either a success or failure message

### `!remove`

When a viewer sends:

```text
!remove
```

AppleCrap removes that viewer’s **most recent active request** from the queue and replies in chat.

---

## Queue Moderation Logic

Every request goes through validation before it is accepted.

### Rule Categories

AppleCrap currently enforces:

- maximum total queue size
- maximum active requests per user
- cooldown between requests from the same user
- duplicate-track blocking
- maximum allowed song duration
- optional link blocking
- optional moderator/broadcaster bypass

### Moderator Bypass

If `modsBypassLimits` is enabled, Twitch moderators and the broadcaster can skip normal limits such as queue size per user and cooldown checks.

### Queue Ordering

Accepted requests are appended to the **back** of the queue.

That means:

- the oldest active request is at the top
- the top request is the next handoff item
- `!remove` removes the requester’s newest item, not the oldest one

---

## Apple Music Request Resolution

AppleCrap supports two main request styles:

- plain text requests
- direct Apple Music track links

### Plain Text Resolution

For text queries, AppleCrap uses Apple’s public iTunes Search API and scores results based on:

- title match quality
- artist match quality
- album overlap
- token overlap
- ordering of title and artist terms
- penalties for terms like `live`, `remix`, `karaoke`, or `version` when they were not requested

This allows noisy viewer input like:

```text
t-pain booty werk
```

to resolve to the intended Apple Music track.

### Direct Apple Music Link Resolution

If link requests are enabled and the viewer submits an Apple Music track URL like:

```text
https://music.apple.com/us/album/freefall-feat-durand-bernarr/1490035834?i=1490036368
```

AppleCrap:

- parses the URL
- extracts the track id from the `i=` query parameter
- uses Apple’s public lookup endpoint to fetch the exact track
- converts it into the app’s standard normalized track object
- marks it as a matched request rather than manual review

This is important because it skips the ambiguity of search and preserves the exact song the user linked.

### What Counts as a Match

A resolved track contains:

- `id`
- `title`
- `artistName`
- `albumName`
- `durationMs`
- Apple Music URL
- optional artwork URL

When a request has a resolved track, it is marked as:

- `matched`

If resolution fails, the request can still be accepted as:

- `manual-review`

depending on the surrounding request rules and input type.

---

## Apple Music Handoff Model

AppleCrap does **not** inject songs directly into the native Apple Music queue.

Instead, it provides a handoff model:

- the top request is shown in a large featured panel
- the streamer clicks `Open In Apple Music`
- Apple Music opens to the exact song URL or a search page fallback
- the streamer starts playback manually

This is the fundamental compromise that makes the app possible on Windows without relying on undocumented Apple queue APIs.

---

## Featured Request Panel

The “On Deck” panel is the main operational surface of the app.

It shows:

- resolved headline: `Song Title - Artist`
- requester username
- request status
- submission timestamp
- album and track duration when available
- `Open In Apple Music`
- playback check / confirmation state

If the request is unresolved, the panel still shows the raw query and instructs the streamer to review/open it manually.

If the queue is empty, the panel switches to an idle state with a rotating set of messages such as:

- `Where the music at, chat?`
- `Nothing on deck yet. Wake chat up.`
- `Remind your chat to use {command} to request songs!`
- `The line is clear. Toss a song into the mix.`

Those messages rotate only when:

- the queue clears after previously containing items
- the queue is manually cleared

They do not animate on a timer.

---

## Remaining Queue List

Below the featured request panel is a compact list of the remaining queued songs.

For each entry, the UI shows:

- title
- requester
- status (`Ready` or `Review`)
- actions:
  - `Open`
  - `Remove`

If there is a featured request but nothing behind it, the list shows:

- `No songs after the current pick.`

If there is no featured request, the lower list stays visually quiet instead of repeating redundant “no requests” messaging.

---

## Now Playing Detection

This is one of the most important technical features in the app.

Because AppleCrap cannot push directly into Apple Music’s queue, it compensates by watching playback state on Windows and automatically clearing the top request when the matched song starts playing.

### What It Watches

AppleCrap runs a periodic Windows probe that attempts to detect visible playback sessions for:

- Apple Music
- Spotify
- Windows Media Player / Media Player

### Detection Strategy

The app uses Windows UI Automation through PowerShell to inspect player windows and read:

- track title
- artist
- album
- rough playback status

### Why This Exists

The user workflow is:

- open the top request in Apple Music
- start playback
- let AppleCrap notice it
- let AppleCrap remove the request automatically

That last step is what makes the whole queue feel alive.

### Match Logic

AppleCrap compares:

- the currently detected title and artist
- the current top queue item

It normalizes text and uses fuzzy token overlap so it can still confirm a match even if the labels differ slightly.

### Failure Behavior

If the now-playing probe fails:

- the app sets now-playing state to unavailable
- the log records a short human-readable warning
- the queue still works
- only auto-clear confirmation is degraded

The app is designed so that now-playing problems do not take down the request queue.

---

## Activity Log

AppleCrap keeps a rolling in-app event log for operational visibility.

Examples of things logged:

- bot connected
- bot disconnected
- request accepted
- request removed
- manual review request queued
- Apple Music lookup failures
- now-playing session changes
- now-playing probe failures

The log is capped in memory/persistence so it remains useful without growing forever.

---

## Testing Tools

The app contains a manual testing modal so the main UI can stay streamer-friendly.

The testing panel currently includes:

- manual viewer name input
- manual request text input
- `Add Test Request`
- `Reset Bot Setup`
- `Clear Queue`
- reminders for the current request command and the `!remove` command

This is mainly for local iteration and sanity testing without needing live Twitch chat messages.

---

## Rules Panel

The Rules modal is the post-setup place to adjust moderation behavior without reopening the entire wizard.

It lets the streamer change:

- queue size
- per-user limit
- cooldown
- max track minutes
- duplicate policy
- link policy
- mod bypass

Saving rules updates the persisted app settings immediately.

---

## About Panel

The About modal is lightweight and brand-oriented.

It currently states:

- AppleCrap is the Twitch-to-Apple Music handoff desk built by KerdyLives

It also includes social links for:

- Twitch
- YouTube
- TikTok
- Instagram

using the `kerdylives` handle.

---

## Persistence and Local Data

AppleCrap stores state locally on the Windows machine.

Persisted data includes:

- app settings
- queue state
- recent log history

The state file lives under Electron’s app data directory, in the user profile area used by the packaged app.

This is why:

- settings survive restarts
- queue and logs can persist between launches
- reinstalling over the same install usually keeps the user’s operational state

---

## Packaging and Distribution

AppleCrap is packaged with `electron-builder`.

### Development

Run the app locally with:

```bash
npm install
npm run dev
```

### Windows Installer

Build the installer with:

```bash
npm run pack:win
```

This produces an NSIS installer in:

- `release/`

### Portable Build

Build a portable executable with:

```bash
npm run pack:portable
```

### Current Product Identity

Relevant packaging metadata includes:

- product name: `AppleCrap`
- app id: `com.kerdylives.applecrap`

The installer is versioned using `package.json`.

---

## Architecture

AppleCrap is a small multi-layer desktop app.

### Renderer Layer

Built with:

- React
- TypeScript
- Vite

Responsibilities:

- visual UI
- modals
- setup wizard
- queue display
- user interactions
- status messaging

### Electron Main Process

Responsibilities:

- app lifecycle
- window creation
- menu wiring
- IPC endpoints
- shell integrations
- Edge InPrivate launcher
- clipboard helpers
- persistence coordination

### Domain Services

Key service modules include:

- Twitch bot service
- Apple Music lookup/search service
- state manager
- now-playing probe service

### Preload Bridge

The preload layer exposes a constrained API to the renderer so the UI can:

- read app state
- update settings
- start and stop the bot
- create manual requests
- clear or remove queue items
- open external URLs
- copy text
- open the Twitch token generator in private mode
- receive state updates
- receive menu actions

This keeps the renderer isolated from raw Node/Electron access.

---

## Important Internal Behaviors

### State Manager

The state manager owns:

- settings
- queue
- logs
- bot status
- now-playing state

It is the central source of truth for app state.

### Twitch Bot Service

The bot service:

- validates that required credentials exist
- joins the configured Twitch channel
- listens for messages
- parses request and remove commands
- pushes accepted requests through the state manager
- writes connection/disconnection events to the log

### Apple Music Service

The Apple Music service:

- searches Apple’s public catalog for text input
- ranks candidates
- resolves direct Apple Music track links by track id
- returns normalized track objects the rest of the app can use

### Now Playing Service

The now-playing service:

- runs on an interval
- calls a PowerShell UI Automation probe
- summarizes visible media sessions
- chooses a preferred active session
- compares it against the top queue item
- clears the top item when playback is confirmed

---

## Why AppleCrap Exists Instead of a Simpler Solution

If Apple Music on Windows had:

- a clean public queue API
- streamer-friendly auth
- reliable remote control hooks

this app would be much simpler.

It does not.

So AppleCrap is the result of engineering around platform reality:

- public Apple search/lookup is available
- queue control is not
- Windows UI and playback inspection are possible
- streamers still want the workflow anyway

That combination is exactly why AppleCrap is not “just another chatbot.”

---

## Known Limitations

AppleCrap is intentionally practical, but it has hard limits.

### 1. No True Apple Music Queue Injection

The app cannot directly add a song to the native Apple Music queue in a supported, public, stable way.

### 2. Now Playing Detection Is Best-Effort

The Windows media probe depends on what the player exposes through its visible UI or automation tree.

That means it can break due to:

- app updates
- localization differences
- OS differences
- UI automation weirdness

### 3. Link Resolution Is Track-Focused

Direct Apple Music URL handling is designed for identifiable track links, not full albums or playlists.

### 4. Twitch Bot Credentials Are Local

This is a local desktop app, so setup happens per machine and per Windows user profile.

### 5. Windows-First Product

The current app is built and packaged around Windows desktop behavior.

---

## Troubleshooting Notes

Common things that can go wrong:

### Bot Won’t Connect

Possible causes:

- wrong channel name
- wrong bot username
- missing `oauth:` prefix
- token generated while signed into the wrong Twitch account

### Requests Go to Manual Review

Possible causes:

- text query was too ambiguous
- link requests are disabled
- the link is not a valid Apple Music track URL
- Apple’s public endpoints did not return a usable track

### Queue Does Not Auto-Clear

Possible causes:

- Apple Music playback was not detected by Windows automation
- a different player session was selected first
- title/artist mismatch was too large
- the probe failed on that machine

### Token Generator Flow Feels Weird

That is exactly why AppleCrap now includes:

- `Open InPrivate`
- `Copy Link`

to keep streamers from logging out of their main Twitch account in a normal browser session.

---

## File-Level Orientation

If a developer needs to jump into the codebase quickly, these files matter most:

- `src/App.tsx`
  - main UI
  - wizard
  - queue rendering
  - modals
  - primary interactions

- `src/App.css`
  - full visual presentation
  - custom frame
  - queue layout
  - modal styling

- `electron/state.mjs`
  - state persistence
  - queue logic
  - request validation
  - moderation rules

- `electron/twitch-bot.mjs`
  - Twitch connection and command parsing

- `electron/apple-music.mjs`
  - Apple search ranking
  - Apple Music link resolution

- `electron/now-playing.mjs`
  - playback detection
  - auto-clear logic

- `electron/main.mjs`
  - Electron window
  - menu setup
  - IPC endpoints
  - desktop integrations

- `electron/preload.cjs`
  - safe renderer bridge

---

## The Honest Product Summary

AppleCrap is a streamer tool for Apple Music users who need song requests badly enough to accept a smart handoff workflow instead of full queue automation.

It is successful when:

- Twitch chat can request songs naturally
- the streamer sees the next request clearly
- AppleCrap resolves most requests to the right Apple Music track
- the streamer can open and play the song fast
- the queue clears itself once playback starts

That is the whole product philosophy:

**make Apple Music song requests usable on Windows, even if the platform refuses to make them elegant.**
