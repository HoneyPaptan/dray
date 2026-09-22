//! Quitting is confirmed in-app, so every route out has to reach the frontend
//! first.
//!
//! There are two, and only one of them is an ordinary window event. Closing the
//! window fires `CloseRequested`, which can be prevented. ⌘Q does not: macOS
//! sends the predefined Quit item straight to `NSApplication.terminate`, and
//! Tauri emits no `ExitRequested` for it — so the app menu is rebuilt here with
//! a *custom* Quit item carrying the same accelerator, which does arrive as a
//! menu event. That is the whole reason this app builds its own menu rather
//! than taking `Menu::default`.
//!
//! One route stays unguarded and cannot be closed: Quit from the Dock's context
//! menu bypasses the menu bar entirely.

use std::sync::Mutex;

use tauri::{menu::Menu, AppHandle, Emitter, Manager, Runtime};

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

/// Mirrors Tauri's default macOS menu with one substitution: Quit is a custom
/// The window's close button is the only route out, and it already arrives as
/// a preventable event — so the default menu is left alone.
pub fn menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    Menu::default(app)
}

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
