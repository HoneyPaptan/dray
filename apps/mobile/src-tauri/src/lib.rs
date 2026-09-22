//! The Dray phone client.
//!
//! A shell and nothing else. Every command the frontend makes travels over a
//! websocket to a `dray` running on the reader's laptop, so there is no agent
//! runtime, no git, no audio and no session store in this process — none of
//! which could cross to Android anyway, and the agent CLIs cannot run on a
//! phone at all. That is the whole reason the phone is a client.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running the Dray phone client");
}
