use std::sync::Arc;

use tauri::State;

use crate::{
    app::AppContext,
    models::{ApproveRequestPayload, ManualRequestPayload, OpenTrackPayload, SaveSettingsPayload},
};

#[tauri::command]
pub async fn bootstrap_app(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    Ok(context.snapshot().await)
}

#[tauri::command]
pub async fn save_settings(
    payload: SaveSettingsPayload,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .save_settings(payload)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_bot(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .connect_bot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disconnect_bot(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .disconnect_bot()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn enqueue_manual_request(
    payload: ManualRequestPayload,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context
        .enqueue_manual_request(&payload.requested_by, &payload.query)
        .await)
}

#[tauri::command]
pub async fn remove_request(
    id: String,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    Ok(context.remove_request(&id).await)
}

#[tauri::command]
pub async fn clear_queue(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    Ok(context.clear_queue().await)
}

#[tauri::command]
pub async fn search_apple_music(
    query: String,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::SearchResult, String> {
    context
        .search_apple_music(&query)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_track(
    payload: OpenTrackPayload,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context.open_track(payload).await)
}

#[tauri::command]
pub async fn run_probe(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::ProbeResult, String> {
    context
        .run_probe_once()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dispatch_next_request(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .dispatch_next_request()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn approve_request(
    payload: ApproveRequestPayload,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .approve_request(payload)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn send_request_to_manual_review(
    id: String,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .send_request_to_manual_review(&id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn export_diagnostics(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context.export_diagnostics().await)
}

#[tauri::command]
pub async fn reveal_data_folder(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context.reveal_data_folder().await)
}

#[tauri::command]
pub async fn import_legacy_state(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context.import_legacy_state().await)
}

#[tauri::command]
pub async fn check_for_updates(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .check_for_updates()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_update(
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context.install_update().await)
}

#[tauri::command]
pub async fn player_bridge_report(
    payload: serde_json::Value,
    context: State<'_, Arc<AppContext>>,
) -> Result<(), String> {
    context.player_bridge.handle_report(payload).await;
    Ok(())
}

#[tauri::command]
pub fn player_show(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let window = app
        .get_webview_window(crate::services::player_bridge::PLAYER_WINDOW_LABEL)
        .ok_or_else(|| "The player window is not available.".to_string())?;
    crate::services::player_bridge::set_player_rendering(&window, true);
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn player_hide(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let window = app
        .get_webview_window(crate::services::player_bridge::PLAYER_WINDOW_LABEL)
        .ok_or_else(|| "The player window is not available.".to_string())?;
    window.hide().map_err(|error| error.to_string())?;
    crate::services::player_bridge::set_player_rendering(&window, false);
    Ok(())
}

#[tauri::command]
pub fn window_minimize(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_toggle_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())
    } else {
        window.maximize().map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn window_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_start_drag(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|error| error.to_string())
}
