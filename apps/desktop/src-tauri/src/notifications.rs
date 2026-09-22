use tauri::{AppHandle, Emitter, Manager};

/// Show a desktop notification that clicks back into the session it came from.
///
/// Deliberately not `tauri-plugin-notification`: it drops the handle `show`
/// returns, and that handle is the only thing a click is reported through. Its
/// own `onAction` listener is wired to an event only the mobile backends emit,
/// so no configuration makes the desktop path deliver one.
///
/// Waiting on the handle blocks until the reader acts or the banner ages out,
/// hence `spawn_blocking`: one parked thread per banner on screen, bounded by
/// how many the OS will stack.
#[tauri::command]
pub async fn notify_session(
    app: AppHandle,
    session_id: String,
    kind: String,
    title: String,
    body: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(&title)
            .body(&body)
            .auto_icon()
            // The freedesktop server reports a click on the banner body as
            // `"default"` only for an app that declared that action, so without
            // this the wait below can only ever be told the banner closed — and
            // clicking through to the session would do nothing at all.
            .action("default", "Open")
            .sound_name(sound_for(&kind));

        // Critical is what keeps a question on screen until it is answered:
        // GNOME expires `Normal` after a few seconds, which is right for a turn
        // that has finished and wrong for one that is still holding.
        if kind == "asking" {
            notification.urgency(notify_rust::Urgency::Critical);
        }

        let handle = match notification.show() {
            Ok(handle) => handle,
            // Best-effort by design: the in-app notice and the sidebar rail both
            // survive this, so a failure must never reach the reader as an error.
            Err(e) => return eprintln!("[notify err] {e}"),
        };

        // Not per-platform: notify-rust reports through one signature on every
        // backend and normalises both names this reads, so the macOS handle and
        // the freedesktop one answer the same two words.
        handle.wait_for_action(|action| {
            // A tap on the banner body is `"default"`; a dismissal or an expiry
            // is `"__closed"`, which is the reader declining to look — raising
            // the window on that would be the opposite of what was asked.
            if action != "__closed" {
                activate(&app, &session_id);
            }
        });
    });

    Ok(())
}

/// The banner's sound, in the freedesktop sound theme's vocabulary.
///
/// A banner with no sound at all is delivered silently — notify-rust only calls
/// `setSound` when a name is set — and silence is worst on exactly this
/// channel, which fires when the reader is in another app and can neither see
/// the in-app notice nor hear the sound it plays.
///
/// These are theme *names*, not files, and an unknown one is silently ignored,
/// so both are taken from the naming spec's own list rather than invented. The
/// two kinds are told apart because only one of them is a request: a question
/// left unanswered holds the session open.
fn sound_for(kind: &str) -> &'static str {
    match kind {
        "asking" => "message-new-instant",
        _ => "complete",
    }
}

/// Bring the window forward and tell the frontend which session was asked for.
///
/// Both halves are needed and neither implies the other: the OS raises the app
/// on its own, but nothing about being frontmost selects a session.
fn activate(app: &AppHandle, session_id: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Err(e) = app.emit("notification_activated", session_id) {
        eprintln!("[notify emit err] {e}");
    }
}
