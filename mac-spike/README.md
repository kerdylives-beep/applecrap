# Crabapple Mac spike

A throwaway test app, **not** Crabapple itself. It answers one question:
can Apple Music play full songs inside a Tauri app on a Mac? Crabapple
on Windows plays music through music.apple.com inside its own window. On a Mac,
that window is Apple's WebKit (WKWebView), and Apple Music's songs are protected
with FairPlay DRM, which may refuse to play in third-party apps.

- `src-tauri/src/main.rs`: a launcher window with two buttons that open
  music.apple.com, either with the default WKWebView user agent or with a Safari
  user agent. Sign-in popups are allowed.
- `src-tauri/src/spike.js`: injected into music.apple.com. It shows a
  red-bordered panel with the user agent, DRM (EME/FairPlay) support, MusicKit
  state, a **Play test song** button (Human Nature by Michael Jackson, the app's
  health-check song) and a log of every playback event and error.
- Built by `.github/workflows/mac-spike.yml` on GitHub's macOS runner, as an
  ad-hoc-signed universal app (Apple Silicon and Intel). No releases.

## Testing it on the Mac

1. Download: open the latest **Mac spike** run under the repo's Actions tab,
   scroll to **Artifacts**, and download **Crabapple-Mac-Spike**. You'll get
   `Crabapple-Mac-Spike.zip`; double-click it (and the zip inside if there is
   one) to get **Crabapple Mac Spike.app**. Drag it to Applications.
2. Open it once. macOS says it "can't be opened" or "Apple could not verify…".
   Click **Done** (not Move to Trash).
3. Open **System Settings → Privacy & Security**, scroll down to Security, and
   click **Open Anyway** next to "Crabapple Mac Spike was blocked". Confirm with
   **Open Anyway** and your password. The app opens.
4. In the spike window, click **1. Open Apple Music**. Sign in with your Apple
   Account (top-right **Sign In** on the Apple Music page). Allow any popup.
5. In the red-bordered panel at the top right, click **Play test song**. Wait
   15 seconds. Listen: do you hear the song?
6. Click **Copy report** and paste the report somewhere (Notes is fine). Also
   take a screenshot of the window (Cmd+Shift+4, then Space, then click the
   window).
7. If the song did not play: close that window, click
   **2. Open with Safari user agent** in the spike window, and repeat steps 5–6.

## What to report back

- Did you hear the song with button 1? With button 2?
- Did signing in work, or did the sign-in popup fail?
- The copied report(s) and screenshot(s).
- Your Mac model (Apple Silicon or Intel) and macOS version
  ( → About This Mac).
