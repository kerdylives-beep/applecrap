use std::sync::Arc;

use tauri::State;

use crate::{
    app::AppContext,
    models::{
        ApproveRequestPayload, ManualRequestPayload, OpenTrackPayload, RunAutomationPayload,
        SaveSettingsPayload,
    },
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
pub async fn run_automation_step(
    payload: RunAutomationPayload,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::CommandResult, String> {
    Ok(context.run_automation(payload).await)
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
pub async fn set_dispatch_hotkey(
    shortcut: String,
    context: State<'_, Arc<AppContext>>,
) -> Result<crate::models::AppState, String> {
    context
        .set_dispatch_hotkey(shortcut)
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
