# Crabapple landing page

A single static page: `index.html` plus the crab artwork. No build step and no dependencies
besides Google Fonts. To preview it, run `python -m http.server` in this folder and open
http://localhost:8000.

- The page scrolls through six scenes, then a hidden seventh. Scroll position drives every
  animation; the code is the script at the bottom of `index.html`.
- The easter egg at the end plays a short synthesized tune (written for this page, generated in
  the browser with the Web Audio API). No audio files are shipped.
- The overlay mockups copy the styles in `src-tauri/src/scripts/overlay.html`. If that file's look
  changes, update the mockups to match.
- `.github/workflows/site.yml` publishes this folder with GitHub Pages when `site/` changes on `main`.
