/// Whether to draw ⌘ or Ctrl in a shortcut hint.
///
/// Rendered rather than detected per-keystroke: [useHotkey](../hooks/useHotkey.ts)
/// accepts either modifier, so this only decides which symbol a tooltip shows.
export const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/// Whether the OS floats its own window controls over the app's titlebar row.
///
/// macOS alone. `titleBarStyle: "Overlay"` leaves the traffic lights sitting on
/// top of the header this app draws, which is why every titlebar row reserves
/// `--traffic-lights-w` at its leading edge. Linux runs `decorations: false`
/// (see tauri.linux.conf.json) and draws its own from
/// [WindowControls](../components/WindowControls.tsx), so there is nothing to
/// clear on the left and the controls land on the right instead.
export const TRAFFIC_LIGHTS = IS_MAC;

/// Whether the app draws the window's own minimise, maximise and close.
///
/// The inverse of the above rather than a second reading of the platform, so
/// the two can never disagree about who owns the chrome.
export const OWN_WINDOW_CONTROLS = !TRAFFIC_LIGHTS;

/// True where nothing native sits at the leading edge of a titlebar row.
///
/// macOS in fullscreen has no traffic lights, and off macOS there were never
/// any — one question, so the layouts that answer it stay one branch rather
/// than growing a platform check beside a fullscreen check at every site.
export function leadingEdgeFree(fullscreen: boolean) {
  return fullscreen || !TRAFFIC_LIGHTS;
}
