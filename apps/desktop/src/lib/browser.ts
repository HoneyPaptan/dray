import { call, subscribeEvent } from "@/lib/transport";
import { useSyncExternalStore } from "react";

/// The in-app browser's frontend half: tabs per session as the backend
/// reports them, and the frames it draws them with.
///
/// The page is a headless Chromium on the machine running the runtime, and
/// what arrives here is its screencast — one JPEG per change, over the same
/// event pipe every other event takes, so the desktop pane and a phone draw
/// it the same way. Input goes back as CDP's own events in device
/// coordinates; the pane knows the size it asked for, so it owns the mapping.

export type BrowserTab = {
  id: number;
  url: string;
  title: string;
  loading: boolean;
  active: boolean;
};

/// One screencast frame: a data URL and the CSS size the page was laid out
/// at when it was taken, which is what a pointer position is mapped onto.
export type Frame = { src: string; width: number; height: number };

const EMPTY: BrowserTab[] = [];
const tabsBySession = new Map<string, BrowserTab[]>();
const framesBySession = new Map<string, Frame>();
const fetched = new Set<string>();
const listeners = new Set<() => void>();
let started = false;

function notify() {
  for (const l of listeners) l();
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => void listeners.delete(l);
}

function start() {
  if (started) return;
  started = true;
  void subscribeEvent<{ sessionId: string; tabs: BrowserTab[] }>("browser_tabs", (e) => {
    const { sessionId, tabs } = e.payload;
    // A tab arriving is what the pending new tab was waiting for, whoever
    // opened it — the URL bar, a link in the chat, a popup.
    if (tabs.length > (tabsBySession.get(sessionId)?.length ?? 0)) {
      pending.delete(sessionId);
      openErrors.delete(sessionId);
    }
    if (tabs.length === 0) framesBySession.delete(sessionId);
    tabsBySession.set(sessionId, tabs);
    fetched.add(sessionId);
    notify();
  });
  void subscribeEvent<{ sessionId: string; tab: number; data: string; width: number; height: number }>(
    "browser_frame",
    (e) => {
      const { sessionId, data, width, height } = e.payload;
      framesBySession.set(sessionId, { src: `data:image/jpeg;base64,${data}`, width, height });
      notify();
    },
  );
}

function fetchTabs(sessionId: string) {
  if (fetched.has(sessionId)) return;
  fetched.add(sessionId);
  void call<BrowserTab[]>("browser_tabs", { sessionId })
    .then((tabs) => {
      tabsBySession.set(sessionId, tabs);
      notify();
    })
    .catch(() => fetched.delete(sessionId));
}

/// `null` until the session's first read answers, so a caller acting on a
/// tab *appearing* can tell one from the list just being learned.
export function useBrowserTabs(sessionId: string | null): BrowserTab[] | null {
  start();
  if (sessionId) fetchTabs(sessionId);
  return useSyncExternalStore(subscribe, () =>
    sessionId ? (tabsBySession.get(sessionId) ?? null) : EMPTY,
  );
}

/// The newest frame of the session's active tab, or `null` before one lands.
export function useFrame(sessionId: string): Frame | null {
  start();
  return useSyncExternalStore(subscribe, () => framesBySession.get(sessionId) ?? null);
}

/// Why the last open in a session failed, or `null`. Held here rather than
/// in the pane, so every route that opens — the URL bar, a local server
/// row, a link in the chat — reports through one place, and the next open
/// or a dismissed new tab clears it.
const openErrors = new Map<string, string>();

export function useOpenError(sessionId: string): string | null {
  return useSyncExternalStore(subscribe, () => openErrors.get(sessionId) ?? null);
}

export function openInBrowser(sessionId: string, url: string, newTab = false) {
  openErrors.delete(sessionId);
  notify();
  return call("browser_open", { sessionId, url, newTab }).catch((e: unknown) => {
    openErrors.set(sessionId, String(e));
    notify();
    throw e;
  });
}

export function activateTab(sessionId: string, id: number) {
  return call("browser_activate", { sessionId, id });
}

export function closeTab(sessionId: string, id: number) {
  return call("browser_close", { sessionId, id });
}

export function navigate(sessionId: string, action: "back" | "forward" | "reload" | "stop" | "hard_reload") {
  return call("browser_nav", { sessionId, action });
}

// --- The screencast ----------------------------------------------------------

export type StageSize = { width: number; height: number; scale: number; touch: boolean };

/// Who is drawing a session's page. Two mounts can — the panel's Browser tab
/// and the full view — and only the one on screen claims; the screencast
/// runs while anybody claims and stops a moment after the last claim goes.
/// The moment is what lets one mount hand over to the other without the
/// page going dark in between: the panel's release and the full view's
/// claim land in one commit, and a stop sent on the release would race the
/// start that follows it.
const stages = new Map<string, { sessionId: string; size: StageSize }>();
const stopTimers = new Map<string, ReturnType<typeof setTimeout>>();

export function claimStage(key: string, sessionId: string, size: StageSize) {
  stages.set(key, { sessionId, size });
  const timer = stopTimers.get(sessionId);
  if (timer) {
    clearTimeout(timer);
    stopTimers.delete(sessionId);
  }
  void call("browser_watch", { sessionId, ...size }).catch(() => undefined);
}

export function releaseStage(key: string) {
  const claim = stages.get(key);
  if (!claim) return;
  stages.delete(key);
  const { sessionId } = claim;
  if ([...stages.values()].some((s) => s.sessionId === sessionId)) return;
  stopTimers.set(
    sessionId,
    setTimeout(() => {
      stopTimers.delete(sessionId);
      if ([...stages.values()].some((s) => s.sessionId === sessionId)) return;
      void call("browser_unwatch", { sessionId }).catch(() => undefined);
    }, 300),
  );
}

/// One CDP input event onto the session's active tab. Fire and forget: a
/// pointer move that fails has nothing to report, and the next one is a
/// frame away.
export function sendInput(sessionId: string, method: string, params: Record<string, unknown>) {
  void call("browser_input", { sessionId, method, params }).catch(() => undefined);
}

// --- The pending new tab -----------------------------------------------------

/// A new tab is nothing until it has a URL: no page is opened for it, so
/// the pane draws its own empty state where the page would be. One per
/// session, and it turns into a real tab the moment one arrives.
const pending = new Set<string>();

export function usePendingTab(sessionId: string): boolean {
  return useSyncExternalStore(subscribe, () => pending.has(sessionId));
}

export function setPendingTab(sessionId: string, on: boolean) {
  if (on) pending.add(sessionId);
  else {
    pending.delete(sessionId);
    openErrors.delete(sessionId);
  }
  notify();
}

// --- Local servers -----------------------------------------------------------

export type LocalServer = { port: number; process: string; mine: boolean };

export function listLocalServers(sessionId: string) {
  return call<LocalServer[]>("list_local_servers", { sessionId });
}

// --- Device viewport ---------------------------------------------------------

export type Viewport = { preset: string; width: number; height: number };

export const VIEWPORT_PRESETS: readonly { id: string; label: string; width: number; height: number }[] = [
  { id: "iphone-se", label: "iPhone SE", width: 375, height: 667 },
  { id: "iphone-15", label: "iPhone 15", width: 393, height: 852 },
  { id: "pixel-8", label: "Pixel 8", width: 412, height: 915 },
  { id: "ipad-mini", label: "iPad Mini", width: 768, height: 1024 },
  { id: "ipad-air", label: "iPad Air", width: 820, height: 1180 },
  { id: "laptop", label: "Laptop", width: 1280, height: 800 },
  { id: "desktop", label: "Desktop", width: 1440, height: 900 },
];

const viewportBySession = new Map<string, Viewport>();

export function useViewport(sessionId: string): Viewport | null {
  return useSyncExternalStore(subscribe, () => viewportBySession.get(sessionId) ?? null);
}

/// `null` is the responsive default: the page fills the pane.
export function setViewport(sessionId: string, viewport: Viewport | null) {
  if (viewport) viewportBySession.set(sessionId, viewport);
  else viewportBySession.delete(sessionId);
  notify();
}

// --- Mapping the pane onto the page ------------------------------------------

/// CDP's modifier bitmask.
export function modifiersOf(e: { altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): number {
  return (e.altKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0);
}

/// A pointer position inside the drawn frame, in the page's own CSS pixels.
/// The frame may be drawn smaller than the page was laid out at — a preset
/// wider than the pane is clamped — so the ratio is taken from the drawn
/// box, never assumed to be one. Clamped to the page, since a pointer
/// released just past the edge still ends the press it started.
export function pagePoint(
  client: { x: number; y: number },
  box: { left: number; top: number; width: number; height: number },
  page: { width: number; height: number },
): { x: number; y: number } {
  const sx = box.width > 0 ? page.width / box.width : 1;
  const sy = box.height > 0 ? page.height / box.height : 1;
  const x = Math.min(page.width, Math.max(0, (client.x - box.left) * sx));
  const y = Math.min(page.height, Math.max(0, (client.y - box.top) * sy));
  return { x: Math.round(x * 100) / 100, y: Math.round(y * 100) / 100 };
}

/// What a soft keyboard's text field changed, as the keys that would have
/// done it: delete the tail the two values disagree on, then type the new
/// one. Android's IME rewrites the whole word as it corrects, so the field's
/// text is compared rather than trusting the input event's `data`.
export function diffInput(prev: string, next: string): { del: number; add: string } {
  let p = 0;
  while (p < prev.length && p < next.length && prev[p] === next[p]) p++;
  return { del: prev.length - p, add: next.slice(p) };
}

/// What the URL bar opens. A scheme is taken as written; `host:port` looks
/// like a scheme, so a scheme wants its `//`. Loopback hosts read as `http`,
/// since that is what a dev server speaks. Anything that is not a host —
/// a space in it, or no dot — is a search.
export function normalizeUrl(input: string): string {
  const s = input.trim();
  if (!s) return s;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s) || /^(about|data|mailto|blob|chrome):/i.test(s)) return s;
  const host = s.split(/[/?#]/)[0] ?? "";
  const loopback = /^(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0)(:\d+)?$/i.test(host);
  if (loopback) return `http://${s}`;
  if (/\s/.test(s) || !host.includes(".")) {
    return `https://www.google.com/search?q=${encodeURIComponent(s)}`;
  }
  return `https://${s}`;
}
