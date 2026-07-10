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
            let context = Arc::new(AppContext::initialize(app.handle().clone())?);
            context.start_background_services();
            app.manage(context);

            // Embedded Apple Music player. Hidden by default; audio keeps
            // playing while hidden. Closing the window hides it instead so the
            // playback webview is never destroyed mid-stream.
            #[cfg(desktop)]
            {
                let player = tauri::WebviewWindowBuilder::new(
                    app,
                    services::player_bridge::PLAYER_WINDOW_LABEL,
                    tauri::WebviewUrl::External("https://music.apple.com/".parse().unwrap()),
                )
                .title("AppleCrap Player — Apple Music")
                .inner_size(1150.0, 820.0)
                .visible(false)
                .initialization_script(include_str!("scripts/player-bridge.js"))
                .build()?;

                let player_handle = player.clone();
                player.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = player_handle.hide();
                    }
                });

                // The player window intentionally survives its own close (it
                // only hides), so closing the main window must end the app or
                // the player keeps the process alive in the background.
                if let Some(main) = app.get_webview_window("main") {
                    let exit_handle = app.handle().clone();
                    main.on_window_event(move |event| {
                        if let tauri::WindowEvent::Destroyed = event {
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
