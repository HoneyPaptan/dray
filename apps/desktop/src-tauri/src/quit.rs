//! Quitting is confirmed in-app, so every route out has to reach the frontend
//! first.
//!
//! On Linux that is one route: closing the window fires `CloseRequested`, which
//! can be prevented. The app therefore sets **no** menu at all — Tauri installs
//! its default one on macOS alone (`App::build`), so leaving the builder's
//! `.menu()` unset is what keeps GTK from drawing an Edit/Window/Help bar above
//! the app's own titlebar. A custom menu existed to intercept macOS's ⌘Q, which
//! reaches `NSApplication.terminate` without emitting `ExitRequested`; that
//! platform is gone from this fork and the menu went with it.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const QUIT_ID: &str = "quit";

/// Whether a quit is on screen unanswered. It is the escape hatch as much as
/// the bookkeeping: with every exit route intercepted, a frontend that never
/// painted would leave the app unquittable, so a *second* request arriving
/// while the first is still unanswered exits outright. Cancelling clears the
/// flag, so the hatch never opens for someone who simply changed their mind
/// twice.
#[derive(Default)]
pub struct PendingQuit(Mutex<bool>);

/// The event the confirmation dialog listens for. Carries nothing — the dialog
/// asks the same question however the quit was asked for.
pub const QUIT_REQUESTED: &str = "quit_requested";

pub fn request<R: Runtime>(app: &AppHandle<R>) {
    let pending = app.state::<PendingQuit>();
    let mut asked = match pending.0.lock() {
        Ok(guard) => guard,
        // A poisoned lock is no reason to trap someone in the app.
        Err(_) => {
            app.exit(0);
            return;
        }
    };

    if *asked {
        app.exit(0);
        return;
    }

    *asked = true;
    if let Err(e) = app.emit(QUIT_REQUESTED, ()) {
        // Nothing is listening, so nothing will ever confirm.
        eprintln!("[quit request emit err] {e}");
        app.exit(0);
    }
}

/// The answer to the confirmation dialog. Nothing else may call `exit` — every
/// other route out is intercepted so that this one is the only one.
#[tauri::command]
pub fn confirm_quit<R: Runtime>(app: AppHandle<R>) {
    app.exit(0);
}

/// The other answer. Clears the flag so the next ⌘Q asks again rather than
/// taking itself for the escape hatch.
#[tauri::command]
pub fn dismiss_quit<R: Runtime>(app: AppHandle<R>) {
    if let Ok(mut asked) = app.state::<PendingQuit>().0.lock() {
        *asked = false;
    }
}
