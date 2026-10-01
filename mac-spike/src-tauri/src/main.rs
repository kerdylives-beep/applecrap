// Throwaway Mac spike. One question: does music.apple.com's protected
// (FairPlay) playback work inside a Tauri WKWebView in a third-party app?
// The launcher opens Apple Music in a window with spike.js injected; that
// script shows a debug panel and a "Play test song" button.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{webview::NewWindowResponse, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

#[tauri::command]
fn open_player(app: AppHandle, safari_ua: bool) -> Result<(), String> {
    let label = if safari_ua { "player-safari-ua" } else { "player-default-ua" };
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let title = if safari_ua {
        "Apple Music (Safari user agent)"
    } else {
        "Apple Music (default WKWebView user agent)"
    };
    let script = include_str!("spike.js").replace("__UA_MODE__", if safari_ua { "safari" } else { "default" });

    let mut builder = WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::External("https://music.apple.com/".parse().unwrap()),
    )
    .title(title)
    .inner_size(1200.0, 860.0)
    .initialization_script(&script)
    // Apple's sign-in opens a popup with window.open. WKWebView blocks popups
    // unless the app allows them; Allow opens a native window sharing this
    // webview's configuration (and so its cookies).
    .on_new_window(|url, _features| {
        println!("[spike] popup requested: {url}");
        NewWindowResponse::Allow
    });

    if safari_ua {
        builder = builder.user_agent(SAFARI_UA);
    }

    builder.build().map(|_| ()).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![open_player])
        .run(tauri::generate_context!())
        .expect("error while running the spike");
}
