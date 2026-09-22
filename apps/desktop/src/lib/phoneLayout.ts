import { useSyncExternalStore } from "react";

/// Where the sidebar stops fitting beside a transcript.
///
/// A width rather than the build, so a narrow desktop window gets the same
/// layout as a phone. The phone build is the reason it exists; it is not the
/// only thing that hits it.
const NARROW = "(max-width: 820px)";

const query = typeof window === "undefined" ? null : window.matchMedia(NARROW);

function watchNarrow(notify: () => void) {
  query?.addEventListener("change", notify);
  return () => query?.removeEventListener("change", notify);
}

/// True while the window is too narrow to hold the sidebar beside the chat.
export function useIsNarrow(): boolean {
  return useSyncExternalStore(
    watchNarrow,
    () => query?.matches ?? false,
    () => false,
  );
}

let drawerOpen = false;
const drawerWatchers = new Set<() => void>();

function watchDrawer(notify: () => void) {
  drawerWatchers.add(notify);
  return () => drawerWatchers.delete(notify);
}

/// Whether the sidebar is drawn over the chat. Narrow layouts only — a wide
/// window has the sidebar in the flow and this says nothing about it.
export function usePhoneDrawer(): boolean {
  return useSyncExternalStore(watchDrawer, () => drawerOpen, () => false);
}

export function setPhoneDrawer(open: boolean) {
  if (drawerOpen === open) return;
  drawerOpen = open;
  for (const watcher of drawerWatchers) watcher();
}

/// Whether this device has a pointer that can hover.
///
/// The question a tooltip actually asks. Width is the wrong one: a narrow
/// desktop window still has a mouse, and a tablet with a keyboard is wide and
/// still has no hover — so a tooltip keyed on width would be missing where it
/// works and present where it cannot open. Touch browsers emulate hover on tap,
/// which is why a tooltip on a phone appears on a press meant for the button
/// under it.
const hoverQuery =
  typeof window === "undefined" ? null : window.matchMedia("(hover: hover) and (pointer: fine)");

function watchHover(notify: () => void) {
  hoverQuery?.addEventListener("change", notify);
  return () => hoverQuery?.removeEventListener("change", notify);
}

export function useCanHover(): boolean {
  return useSyncExternalStore(
    watchHover,
    () => hoverQuery?.matches ?? true,
    () => true,
  );
}

/// The same reading as a constant, for the handful of native `title`
/// attributes left in the app. Read once rather than subscribed: a `title` is
/// not reactive UI, and a device that grows a mouse mid-session is worth less
/// than a hook at every one of those sites.
export const CAN_HOVER = hoverQuery?.matches ?? true;
