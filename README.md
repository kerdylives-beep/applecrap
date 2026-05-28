# AppleCrap Alpha

Portable Windows alpha for Twitch-driven Apple Music song requests.

## What this rewrite is

- Tauri-first desktop rewrite with a React/TypeScript frontend and Rust service layer.
- Windows-only alpha focused on the reliable workflow:
  Twitch request -> Apple Music resolution -> manual handoff -> playback confirmation -> auto-clear.
- Steam-inspired control surface with a queue hero panel, navigation rail, diagnostics inspector, and exportable debug bundles.
- Experimental automation boundary with `deep-link` and `ui-automation` adapters that fail safely without mutating the queue.

## Current project shape

- `src/` contains the new alpha UI, typed command bridge, and client store.
- `src-tauri/` contains the Tauri app shell plus Rust services for persistence, queue logic, Apple Music lookup, playback probing, diagnostics, and Twitch IRC.
- `electron/` is still present as legacy reference material for import/migration behavior.

## Frontend development

```bash
npm install
npm run dev
```

The frontend build is validated with:

```bash
npm run build
npm run lint
npm test
```

## Tauri prerequisites

This machine still needs the Rust/Tauri Windows toolchain before the desktop app itself can be built and run end-to-end:

- Rust toolchain (`rustup`, `cargo`, `rustc`)
- Windows native build tools for Rust on MSVC
- WebView2 runtime on Windows

Once those are installed, use:

```bash
npm run tauri:dev
```

## Portable packaging

The intended alpha output is a zipped folder with no installer.

After the Rust/Tauri toolchain is available and a release executable has been built:

```bash
npm run tauri:build
npm run tauri:portable
```

That packaging script will:

- create `release/portable/AppleCrap Alpha/`
- place the Tauri executable there
- create a sidecar `data/` folder
- add a portable `README.txt`
- zip the folder as `release/portable/AppleCrap Alpha.zip`

## Alpha notes

- Portable storage prefers `./data` beside the executable and falls back to Local AppData if the folder is not writable.
- Legacy Electron data can be imported once into the new alpha store.
- Apple Music queue injection is still experimental; queue removal remains gated on playback confirmation.
