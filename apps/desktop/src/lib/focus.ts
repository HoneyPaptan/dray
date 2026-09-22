import { getCurrentWindow } from "@tauri-apps/api/window";

import { channel } from "@/lib/channel";
import { IS_REMOTE } from "@/lib/transport";

/// Whether the app window is frontmost. Read imperatively rather than as React
/// state: the one caller that matters is inside a Tauri event listener
/// registered once, where a state value would be the mount-time one forever.
///
/// The source is Tauri's own window event, not the DOM's `blur`. A webview
/// blurs whenever focus leaves the document — opening a native menu, clicking
/// into devtools — none of which means the user has left the app, and each
/// would fire a desktop notification at someone looking straight at the window.
/// The DOM pair is the fallback for `pnpm dev`, where there is no Tauri window.
/// On a phone the question is not which window is frontmost but whether the app
/// is on screen at all, and the two have different answers there: Tauri's window
/// focus event does not fire when an Android activity goes to the background, so
/// the app believed it was frontmost for the whole of its life and every
/// notification was withheld as "they are looking right at it". `visibilitychange`
/// is the signal the platform actually sends, and the webview keeps running JS
/// behind it — `WebView.onPause` stops drawing and timers, not scripts — so the
/// socket is still live to notice the turn ending.
const PAGE_VISIBILITY = IS_REMOTE;

let focused = PAGE_VISIBILITY ? document.visibilityState === "visible" : document.hasFocus();

const changed = channel<boolean>();

function set(next: boolean) {
  if (next === focused) return;
  focused = next;
  changed.emit(next);
}

if (PAGE_VISIBILITY) {
  document.addEventListener("visibilitychange", () =>
    set(document.visibilityState === "visible"),
  );
} else {
  try {
    void getCurrentWindow()
      .onFocusChanged(({ payload }) => set(payload))
      .catch(useDomEvents);
  } catch {
    // `getCurrentWindow` reads a global the plain browser doesn't have, so this
    // throws rather than rejecting under `pnpm dev`.
    useDomEvents();
  }
}

function useDomEvents() {
  window.addEventListener("focus", () => set(true));
  window.addEventListener("blur", () => set(false));
}

/// True while the app window is frontmost.
export function isWindowFocused(): boolean {
  return focused;
}

/// Subscribe to focus changes; returns the unsubscribe.
export const onFocusChange = changed.subscribe;
