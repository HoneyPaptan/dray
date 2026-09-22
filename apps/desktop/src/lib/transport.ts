import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { remote } from "./remoteTransport";

/**
 * Where the agent runtime lives. `false` on the desktop build, where every
 * command is an ordinary Tauri call into this same process; `true` on the
 * mobile build, where the runtime is a `dray serve` on another machine and
 * every command travels over a websocket.
 *
 * Build-time rather than runtime, because the mobile shell is itself a Tauri
 * app: "am I inside Tauri" cannot tell the two apart.
 */
export const IS_REMOTE = import.meta.env.VITE_DRAY_REMOTE === "1";

/** Invoke a backend command. The one call every feature goes through. */
export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return IS_REMOTE ? remote.call<T>(cmd, args) : invoke<T>(cmd, args);
}

/**
 * Subscribe to a backend event. Resolves to the unsubscribe function.
 *
 * The handler takes Tauri's own `{ payload }` envelope rather than the payload
 * alone, so a call site reads identically whichever side of the seam it is on.
 */
export function subscribeEvent<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (IS_REMOTE) return remote.subscribe<T>(event, handler);
  return listen<T>(event, handler);
}

/**
 * A URL the webview can fetch for a file on the machine running the runtime.
 * Remote, the asset protocol is unreachable, so the bytes ride the server's
 * own HTTP side instead.
 */
export function assetUrl(path: string): string {
  return IS_REMOTE ? remote.assetUrl(path) : convertFileSrc(path);
}
