import { useEffect, useRef } from "react";

/// What a phone's Back gesture closes, and in what order.
///
/// Android's back is one signal for every "go up a level" a screen has, so the
/// app has to answer it with the *innermost* thing that is open rather than
/// with the activity. Unanswered, the system finishes the activity — which is
/// the whole bug: the reader swipes back out of a settings sheet and the app
/// quits instead.
///
/// The frontend is the only side that knows what is open, so the Android shell
/// asks it ([`installBackHandler`] publishes the answer on `window`) and
/// finishes the activity only when this says there was nothing left to close.

/// A layer that back takes away. Newest closed first.
type Layer = () => void;

const layers: Layer[] = [];

/// Register a layer for as long as it is open. Returns its remover.
export function pushBackLayer(close: Layer): () => void {
  layers.push(close);
  return () => {
    const at = layers.lastIndexOf(close);
    if (at >= 0) layers.splice(at, 1);
  };
}

/// Register while `active`, drop when it goes. `close` is read at the press, so
/// a handler that changes every render does not churn the registration.
export function useBackLayer(active: boolean, close: () => void) {
  const latest = useRef(close);
  latest.current = close;

  useEffect(() => {
    if (!active) return;
    return pushBackLayer(() => latest.current());
  }, [active]);
}

/// Radix draws every menu, dialog and popover into a portal, so what is open
/// can be asked of the DOM rather than of a registration each of them would
/// have to remember to make. A tooltip is excluded: it is the one popper that
/// closes on its own and answering back with it would spend a press on
/// something the reader cannot see change.
const OVERLAYS = '[data-radix-popper-content-wrapper], [role="dialog"], [role="alertdialog"]';

/// Whether a menu, dialog or popover is drawn over the app. Also read by the
/// phone's swipe gestures, which must not move the panes behind an open sheet.
export function hasOpenOverlay(): boolean {
  for (const el of document.querySelectorAll(OVERLAYS)) {
    if (el.querySelector('[role="tooltip"]')) continue;
    return true;
  }
  return false;
}

/// Close the innermost thing that is open. `false` means there was nothing, so
/// the caller may take its own action — quitting, on Android.
///
/// Overlays are asked about first and answered with a synthetic Escape, since
/// they are drawn over everything the stack holds and Radix's own dismiss path
/// is what knows how to unwind focus. Its listener sits on `document` and reads
/// `key` alone, so an untrusted event is enough.
export function handleBack(): boolean {
  if (hasOpenOverlay()) {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    return true;
  }

  const layer = layers.pop();
  if (!layer) return false;
  layer();
  return true;
}

declare global {
  interface Window {
    /// Read by the Android shell's back callback. Its absence is what an older
    /// frontend looks like, and the shell falls back to quitting.
    __drayBack?: () => boolean;
  }
}

/// Publish the answer for the Android shell. Called once for the process.
export function installBackHandler() {
  window.__drayBack = handleBack;
}
