import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { IS_REMOTE } from "./transport";

type RemoteCall = { token: number; cmd: string; args: Record<string, unknown> };

/**
 * Run the commands a phone asks for.
 *
 * Tauri can dispatch a registered command by name only from a webview, so the
 * desktop window is what executes a remote client's call and `serve.rs` only
 * relays. This is deliberately the raw `invoke` rather than the seam's `call`:
 * the seam is what a *remote* frontend uses, and routing through it here would
 * send a phone's command straight back out to the phone.
 */
export function startRemoteExecutor() {
  if (IS_REMOTE) return;
  void listen<RemoteCall>("remote_call", async ({ payload }) => {
    try {
      const value = await invoke(payload.cmd, payload.args);
      await invoke("remote_reply", { token: payload.token, ok: true, value: value ?? null });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      await invoke("remote_reply", { token: payload.token, ok: false, value: message });
    }
  });
}
