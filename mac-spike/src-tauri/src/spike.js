// Crabapple Mac spike: injected into music.apple.com. Shows a debug panel with
// the user agent, DRM (EME) support, MusicKit state and a "Play test song"
// button, and logs everything that happens so the result can be reported.
(() => {
  if (window.top !== window) return; // main frame only
  if (window.__SPIKE__) return;
  window.__SPIKE__ = true;

  const UA_MODE = "__UA_MODE__";
  const log = [];
  const started = Date.now();
  const stamp = () => ((Date.now() - started) / 1000).toFixed(1).padStart(6) + "s";
  const say = (line) => {
    log.push(`${stamp()}  ${line}`);
    if (log.length > 400) log.shift();
    render();
  };
  const errText = (e) => {
    if (!e) return String(e);
    const parts = [e.name, e.errorCode, e.code, e.message, e.description, e.reason].filter((p) => p !== undefined && p !== "");
    try { return parts.length ? parts.join(" | ") : JSON.stringify(e); } catch { return String(e); }
  };

  // Catch everything the page throws, so playback errors are not lost.
  window.addEventListener("error", (e) => say(`page error: ${e.message}`));
  window.addEventListener("unhandledrejection", (e) => say(`unhandled rejection: ${errText(e.reason)}`));
  // DRM events fire on media elements when protected content starts loading.
  for (const type of ["encrypted", "webkitneedkey", "waitingforkey"]) {
    document.addEventListener(type, (e) => say(`media event: ${type} on <${e.target && e.target.tagName}> initDataType=${e.initDataType || "-"}`), true);
  }
  for (const type of ["error", "stalled", "playing", "pause"]) {
    document.addEventListener(type, (e) => {
      const t = e.target;
      if (!(t instanceof HTMLMediaElement)) return;
      const err = t.error ? ` code=${t.error.code} ${t.error.message || ""}` : "";
      say(`<${t.tagName.toLowerCase()}> ${type}${err}`);
    }, true);
  }

  // ---------- checks ----------
  const results = { env: [], eme: [], musickit: [], live: [] };

  function envCheck() {
    results.env = [
      `UA mode: ${UA_MODE}`,
      `userAgent: ${navigator.userAgent}`,
      `has "Safari" token: ${/Safari\//.test(navigator.userAgent)}`,
      `platform: ${navigator.platform}  secureContext: ${window.isSecureContext}`,
      `url: ${location.href}`,
    ];
  }

  async function emeCheck() {
    const out = [];
    out.push(`navigator.requestMediaKeySystemAccess: ${typeof navigator.requestMediaKeySystemAccess}`);
    out.push(`window.MediaKeys: ${typeof window.MediaKeys}   window.WebKitMediaKeys: ${typeof window.WebKitMediaKeys}`);
    out.push(`MediaSource: ${typeof window.MediaSource}   ManagedMediaSource: ${typeof window.ManagedMediaSource}`);
    if (window.MediaSource && MediaSource.isTypeSupported) {
      out.push(`MSE audio/mp4 aac: ${MediaSource.isTypeSupported('audio/mp4; codecs="mp4a.40.2"')}`);
    }
    if (window.WebKitMediaKeys && WebKitMediaKeys.isTypeSupported) {
      for (const ks of ["com.apple.fps", "com.apple.fps.1_0", "com.apple.fps.2_0", "com.apple.fps.3_0"]) {
        for (const mime of ["audio/mp4", "video/mp4"]) {
          let r; try { r = WebKitMediaKeys.isTypeSupported(ks, mime); } catch (e) { r = "threw " + errText(e); }
          out.push(`WebKitMediaKeys.isTypeSupported(${ks}, ${mime}): ${r}`);
        }
      }
    }
    if (navigator.requestMediaKeySystemAccess) {
      const audio = [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }];
      const configs = {
        "audio only": [{ initDataTypes: ["sinf", "skd", "cenc"], audioCapabilities: audio }],
        "audio+video": [{ initDataTypes: ["sinf", "skd", "cenc"], audioCapabilities: audio, videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.42E01E"' }] }],
      };
      for (const ks of ["com.apple.fps", "com.apple.fps.1_0", "com.apple.fps.2_0", "com.apple.fps.3_0", "com.widevine.alpha", "org.w3.clearkey"]) {
        for (const [name, cfg] of Object.entries(configs)) {
          try {
            const access = await navigator.requestMediaKeySystemAccess(ks, cfg);
            let keys = "";
            try { await access.createMediaKeys(); keys = ", createMediaKeys OK"; } catch (e) { keys = `, createMediaKeys FAILED: ${errText(e)}`; }
            out.push(`EME ${ks} (${name}): SUPPORTED${keys}`);
          } catch (e) {
            out.push(`EME ${ks} (${name}): no (${errText(e)})`);
          }
        }
      }
    }
    results.eme = out;
  }

  const mkInstance = () => {
    try { return window.MusicKit && MusicKit.getInstance && MusicKit.getInstance(); } catch { return null; }
  };
  const stateName = (v) => {
    try {
      const states = window.MusicKit && MusicKit.PlaybackStates;
      if (states) for (const [k, n] of Object.entries(states)) if (n === v) return `${k} (${v})`;
    } catch {}
    return String(v);
  };

  function musickitCheck() {
    const mk = mkInstance();
    results.musickit = [
      `window.MusicKit: ${typeof window.MusicKit}   version: ${(window.MusicKit && MusicKit.version) || "-"}`,
      `getInstance(): ${mk ? "yes" : "no"}`,
    ];
    if (mk) {
      results.musickit.push(
        `isAuthorized: ${mk.isAuthorized}   storefrontId: ${mk.storefrontId || "-"}`,
        `playbackState: ${stateName(mk.playbackState)}`,
      );
    }
  }

  let lastTime = null, advancing = 0, stalled = 0;
  function liveCheck() {
    const mk = mkInstance();
    const lines = [];
    if (mk) {
      const item = mk.nowPlayingItem;
      const t = Number(mk.currentPlaybackTime);
      if (lastTime !== null && Number.isFinite(t)) {
        if (t > lastTime + 0.3) advancing++; else if (mk.playbackState === (MusicKit.PlaybackStates || {}).playing) stalled++;
      }
      lastTime = Number.isFinite(t) ? t : lastTime;
      lines.push(`now playing: ${item ? `${item.title} — ${item.artistName}` : "-"}`);
      lines.push(`state: ${stateName(mk.playbackState)}   position: ${Number.isFinite(t) ? t.toFixed(1) + "s" : "-"}   duration: ${mk.currentPlaybackDuration || "-"}`);
      lines.push(`position advanced in ${advancing} checks, stuck while "playing" in ${stalled}`);
    } else {
      lines.push("MusicKit not loaded yet");
    }
    for (const el of document.querySelectorAll("audio, video")) {
      lines.push(`<${el.tagName.toLowerCase()}> t=${el.currentTime.toFixed(1)} paused=${el.paused} ready=${el.readyState} muted=${el.muted} vol=${el.volume} mediaKeys=${el.mediaKeys ? "set" : el.webkitKeys ? "webkitKeys set" : "none"}${el.error ? " error=" + el.error.code : ""}`);
    }
    results.live = lines;
    musickitCheck();
    render();
  }

  // ---------- test playback ----------
  async function findTestSong(mk) {
    const field = shadow.getElementById("song");
    const typed = field.value.trim();
    const fromUrl = typed.match(/[?&]i=(\d+)/) || typed.match(/\/song\/[^/]+\/(\d+)/) || typed.match(/^(\d{6,})$/);
    if (fromUrl) return fromUrl[1];
    const term = typed || "Human Nature Michael Jackson";
    say(`searching the catalog for "${term}"`);
    const sf = mk.storefrontId || "us";
    const res = await mk.api.music(`/v1/catalog/${sf}/search`, { term, types: "songs", limit: 1 });
    const song = res && res.data && res.data.results && res.data.results.songs && res.data.results.songs.data[0];
    if (!song) throw new Error("search returned no songs");
    say(`found ${song.attributes.name} — ${song.attributes.artistName} (id ${song.id})`);
    return song.id;
  }

  async function playTest() {
    const mk = mkInstance();
    if (!mk) return say("Play test song: MusicKit is not loaded on this page yet. Wait for the page to finish loading.");
    if (!mk.isAuthorized) say("warning: not signed in (isAuthorized=false). Full songs need sign-in; expect a preview or an error.");
    advancing = 0; stalled = 0; lastTime = null;
    try {
      const id = await findTestSong(mk);
      say(`setQueue({ song: "${id}" })`);
      await mk.setQueue({ song: id });
      say("setQueue OK, calling play()");
      await mk.play();
      say("play() resolved. Watch 'position' and listen for audio.");
      setTimeout(() => say(`after 10s: position advanced in ${advancing} checks, stuck in ${stalled}, state ${stateName(mk.playbackState)}`), 10000);
    } catch (e) {
      say(`PLAY FAILED: ${errText(e)}`);
    }
  }

  let mkHooked = false;
  function hookMusicKit() {
    const mk = mkInstance();
    if (!mk || mkHooked) return;
    mkHooked = true;
    say("MusicKit instance found; listening to its events");
    const ev = (window.MusicKit && MusicKit.Events) || {};
    for (const name of ["playbackStateDidChange", "mediaPlaybackError", "playbackError", "authorizationStatusDidChange", "nowPlayingItemDidChange", "mediaCanPlay"]) {
      const key = ev[name] || name;
      try {
        mk.addEventListener(key, (e) => {
          let detail = "";
          if (name === "playbackStateDidChange") detail = ` ${stateName(e && e.oldState)} → ${stateName(e && e.state)}`;
          else if (name.includes("Error")) detail = ` ${errText(e)}`;
          else if (name === "nowPlayingItemDidChange") detail = ` ${mk.nowPlayingItem ? mk.nowPlayingItem.title : "-"}`;
          else if (name === "authorizationStatusDidChange") detail = ` isAuthorized=${mk.isAuthorized}`;
          say(`MusicKit event: ${name}${detail}`);
        });
      } catch {}
    }
  }

  // ---------- panel ----------
  let host, shadow;
  function report() {
    return [
      "== Crabapple Mac spike report ==",
      `time: ${new Date().toISOString()}`,
      "", "-- environment --", ...results.env,
      "", "-- DRM / EME --", ...results.eme,
      "", "-- MusicKit --", ...results.musickit,
      "", "-- live --", ...results.live,
      "", "-- log --", ...log,
    ].join("\n");
  }
  function render() {
    if (!shadow) return;
    shadow.getElementById("out").textContent = report();
  }
  function mount() {
    if (host && !host.isConnected && document.body) { document.body.appendChild(host); return; }
    if (host || !document.body) return;
    host = document.createElement("div");
    host.style.cssText = "position:fixed;top:12px;right:12px;z-index:2147483647;";
    shadow = host.attachShadow({ mode: "open" });
    shadow.innerHTML = `
      <style>
        .p{width:460px;max-height:80vh;display:flex;flex-direction:column;gap:8px;padding:12px;border-radius:12px;
           background:#141821;color:#f2efea;font:12px/1.4 -apple-system,system-ui,sans-serif;box-shadow:0 12px 40px rgba(0,0,0,.5);border:2px solid #ee2340}
        .p.min pre,.p.min .row2{display:none}
        h1{margin:0;font-size:14px;display:flex;justify-content:space-between;align-items:center}
        .row,.row2{display:flex;gap:6px;flex-wrap:wrap}
        button{font:600 12px -apple-system,system-ui;padding:6px 10px;border-radius:8px;border:0;cursor:pointer;background:#2a303b;color:#f2efea}
        button.go{background:#ee2340;color:#fff}
        input{flex:1;min-width:0;padding:6px 8px;border-radius:8px;border:1px solid #2a303b;background:#0c0e13;color:#f2efea;font:12px ui-monospace,Menlo,monospace}
        pre{margin:0;overflow:auto;white-space:pre-wrap;word-break:break-word;font:11px/1.45 ui-monospace,Menlo,monospace;background:#0c0e13;padding:8px;border-radius:8px;user-select:text;-webkit-user-select:text}
      </style>
      <div class="p" id="p">
        <h1><span>🦀 Crabapple Mac spike · ${UA_MODE} UA</span><button id="min">–</button></h1>
        <div class="row"><button class="go" id="play">Play test song</button><button id="pause">Pause</button><button id="rerun">Re-run checks</button><button id="copy">Copy report</button></div>
        <div class="row2"><input id="song" placeholder="Song: Human Nature Michael Jackson (or paste an Apple Music song link)"></div>
        <pre id="out"></pre>
      </div>`;
    document.body.appendChild(host);
    shadow.getElementById("play").onclick = playTest;
    shadow.getElementById("pause").onclick = () => { const mk = mkInstance(); if (mk) mk.pause(); };
    shadow.getElementById("rerun").onclick = runChecks;
    shadow.getElementById("min").onclick = () => shadow.getElementById("p").classList.toggle("min");
    shadow.getElementById("copy").onclick = async () => {
      try { await navigator.clipboard.writeText(report()); say("report copied to the clipboard"); }
      catch (e) {
        const range = document.createRange(); range.selectNodeContents(shadow.getElementById("out"));
        const sel = shadow.getSelection ? shadow.getSelection() : window.getSelection();
        sel.removeAllRanges(); sel.addRange(range);
        say(`clipboard refused (${errText(e)}); the report text is selected, press Cmd+C`);
      }
    };
    render();
  }

  async function runChecks() {
    envCheck();
    musickitCheck();
    say("running DRM checks…");
    await emeCheck();
    say("DRM checks done");
  }

  const boot = () => {
    mount();
    runChecks();
    setInterval(() => { mount(); hookMusicKit(); liveCheck(); }, 1000);
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
  else boot();
})();
