mod app;
mod commands;
mod models;
mod services;

use std::sync::Arc;

use app::AppContext;
use commands::*;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            services::updater::clean_stale_artifacts();

            // Media keys are claimed dynamically (see
            // AppContext::sync_media_key_claim); this only installs the
            // handler that routes a press into the embedded player.
            #[cfg(desktop)]
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, shortcut, event| {
                        if event.state() != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                            return;
                        }
                        let Some(op) = app::media_key_op(shortcut) else {
                            return;
                        };
                        if let Some(context) = app.try_state::<Arc<AppContext>>() {
                            let context = context.inner().clone();
                            tauri::async_runtime::spawn(async move {
                                context.handle_media_key(op).await;
                            });
                        }
                    })
                    .build(),
            )?;
            #[cfg(windows)]
            services::audio_session::spawn_session_labeler();
            let context = Arc::new(AppContext::initialize(app.handle().clone())?);
            context.start_background_services();
            app.manage(context);

            // Embedded Apple Music player. Hidden by default; audio keeps
            // playing while hidden. Closing the window hides it instead so the
            // playback webview is never destroyed mid-stream.
            #[cfg(desktop)]
            {
                let mut player_builder = tauri::WebviewWindowBuilder::new(
                    app,
                    services::player_bridge::PLAYER_WINDOW_LABEL,
                    tauri::WebviewUrl::External("https://music.apple.com/".parse().unwrap()),
                )
                .title("AppleCrap Player — Apple Music")
                .inner_size(1150.0, 820.0)
                .visible(false)
                .initialization_script(include_str!("scripts/player-bridge.js"));

                // wry builds a separate WebView2 environment per webview, and
                // WebView2 refuses to create a second environment for the same
                // user data folder with different browser arguments — the
                // window then silently fails to build. Mirror the main
                // window's configured args here (read from config so the two
                // can never drift out of sync).
                #[cfg(windows)]
                {
                    if let Some(args) = app
                        .config()
                        .app
                        .windows
                        .iter()
                        .find(|window| window.label == "main")
                        .and_then(|window| window.additional_browser_args.clone())
                    {
                        player_builder = player_builder.additional_browser_args(&args);
                    }
                }

                let player = player_builder.build()?;

                let player_handle = player.clone();
                player.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = player_handle.hide();
                        services::player_bridge::set_player_rendering(&player_handle, false);
                    }
                });

                // The window starts hidden, so start with rendering off too.
                // Audio and the bridge keep running; only drawing is skipped.
                services::player_bridge::set_player_rendering(&player, false);

                // Chromium only exposes audio output device ids/labels to
                // origins holding microphone permission. Pre-grant it for the
                // player origin so the "Audio output" routing dropdown can
                // list real devices. No microphone is ever opened.
                #[cfg(windows)]
                let _ = player.with_webview(|webview| unsafe {
                    use webview2_com::Microsoft::Web::WebView2::Win32::{
                        ICoreWebView2Profile4, ICoreWebView2_13,
                        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                        COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                    };
                    use windows::core::Interface;

                    let grant = (|| -> windows::core::Result<()> {
                        let core = webview.controller().CoreWebView2()?;
                        let webview13: ICoreWebView2_13 = core.cast()?;
                        let profile = webview13.Profile()?;
                        let profile4: ICoreWebView2Profile4 = profile.cast()?;
                        profile4.SetPermissionState(
                            COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
                            &windows::core::HSTRING::from("https://music.apple.com"),
                            COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                            None,
                        )
                    })();
                    if let Err(error) = grant {
                        eprintln!("audio device permission grant failed: {error}");
                    }
                });

                // The player window intentionally survives its own close (it
                // only hides), so closing the main window must end the app or
                // the player keeps the process alive in the background.
                if let Some(main) = app.get_webview_window("main") {
                    let exit_handle = app.handle().clone();
                    main.on_window_event(move |event| {
                        if let tauri::WindowEvent::Destroyed = event {
                            // Persist anything still sitting in the debounced
                            // write buffer before the process goes away.
                            if let Some(context) =
                                exit_handle.try_state::<Arc<AppContext>>().map(|c| c.inner().clone())
                            {
                                tauri::async_runtime::block_on(context.flush_pending_state());
                            }
                            exit_handle.exit(0);
                        }
                    });
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap_app,
            save_settings,
            connect_bot,
            disconnect_bot,
            enqueue_manual_request,
            remove_request,
            clear_queue,
            search_apple_music,
            open_track,
            run_probe,
            dispatch_next_request,
            approve_request,
            send_request_to_manual_review,
            export_diagnostics,
            reveal_data_folder,
            import_legacy_state,
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_start_drag,
            player_bridge_report,
            player_show,
            player_hide,
            check_for_updates,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running AppleCrap Alpha");
}
