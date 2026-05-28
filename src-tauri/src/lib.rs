mod app;
mod commands;
mod models;
mod services;

use std::sync::Arc;

use app::AppContext;
use commands::*;
use tauri::Manager;

#[cfg(desktop)]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let context = Arc::new(AppContext::initialize(app.handle().clone())?);
            #[cfg(desktop)]
            {
                let hotkey_context = Arc::clone(&context);
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |_app, _shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                let context = Arc::clone(&hotkey_context);
                                tauri::async_runtime::spawn(async move {
                                    let _ = context.dispatch_ready_request_from_hotkey().await;
                                });
                            }
                        })
                        .build(),
                )?;
                if let Ok(shortcut) = Shortcut::try_from(
                    context
                        .current_settings_blocking()
                        .automation
                        .dispatch_hotkey
                        .as_str(),
                ) {
                    let _ = app.global_shortcut().register(shortcut);
                }
            }
            context.start_background_services();
            app.manage(context);
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
            run_automation_step,
            dispatch_next_request,
            approve_request,
            send_request_to_manual_review,
            set_dispatch_hotkey,
            export_diagnostics,
            reveal_data_folder,
            import_legacy_state,
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_start_drag
        ])
        .run(tauri::generate_context!())
        .expect("error while running AppleCrap Alpha");
}
