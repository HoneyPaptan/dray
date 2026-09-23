//! The pane's commands: what the Browser tab calls, on the desktop and from a
//! phone over `serve`.
//!
//! Thin on purpose. Every one of these is a name the frontend already used
//! against the embedded browser, answered now by [`super::backend`] over CDP —
//! so the pane's own code needed no new vocabulary, and the verbs `dray
//! browser` speaks and the buttons the reader presses land on one set of tabs.

use serde_json::Value;

use super::backend as be;

#[tauri::command]
pub async fn browser_tabs(session_id: String) -> Result<Vec<be::TabInfo>, String> {
    Ok(be::tabs_info(&session_id))
}

#[tauri::command]
pub async fn browser_open(session_id: String, url: String, new_tab: bool) -> Result<(), String> {
    be::open(&session_id, url, new_tab).await
}

#[tauri::command]
pub async fn browser_activate(session_id: String, id: i32) -> Result<(), String> {
    be::activate(&session_id, id).await
}

#[tauri::command]
pub async fn browser_close(session_id: String, id: i32) -> Result<(), String> {
    be::close_tab(&session_id, id).await
}

/// `back`, `forward`, `reload`, `hard_reload` or `stop`.
#[tauri::command]
pub async fn browser_nav(session_id: String, action: String) -> Result<(), String> {
    be::nav(&session_id, &action).await
}

/// Starts the screencast the pane draws, laid out at the pane's own size.
/// Frames arrive as `browser_frame` events, so this answers before the first
/// one does.
#[tauri::command]
pub async fn browser_watch(
    session_id: String,
    width: u32,
    height: u32,
    scale: f64,
    touch: bool,
) -> Result<(), String> {
    be::cast(&session_id, width, height, scale, touch).await
}

#[tauri::command]
pub async fn browser_unwatch(session_id: String) -> Result<(), String> {
    be::uncast(&session_id).await;
    Ok(())
}

/// One pointer or key event from the pane, already in device coordinates.
#[tauri::command]
pub async fn browser_input(session_id: String, method: String, params: Value) -> Result<(), String> {
    be::input(&session_id, &method, params).await
}
