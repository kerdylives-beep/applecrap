fn main() {
    // Declaring commands in the app ACL manifest lets capabilities grant them
    // per-window — required so the remote music.apple.com player window can be
    // allowed to call player_bridge_report. Once a manifest exists, every
    // command must be declared and granted (see capabilities/).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "bootstrap_app",
            "save_settings",
            "connect_bot",
            "disconnect_bot",
            "enqueue_manual_request",
            "remove_request",
            "clear_queue",
            "search_apple_music",
            "open_track",
            "run_probe",
            "dispatch_next_request",
            "approve_request",
            "send_request_to_manual_review",
            "export_diagnostics",
            "reveal_data_folder",
            "import_legacy_state",
            "window_minimize",
            "window_toggle_maximize",
            "window_close",
            "window_start_drag",
            "player_bridge_report",
            "player_show",
            "player_hide",
            "check_for_updates",
            "install_update",
        ]),
    ))
    .expect("failed to run tauri-build");
}
