import { invoke } from "@tauri-apps/api/core";

import { call, IS_REMOTE } from "@/lib/transport";

import type { NoticeKind } from "@/hooks/useNotices";

/// Post a desktop notification that clicks back into its session.
///
/// The whole thing lives in Rust ([notifications.rs](../../src-tauri/src/notifications.rs))
/// rather than in `tauri-plugin-notification`, because the click is only
/// reachable from the handle the plugin throws away. The session id travels out
/// and comes back on `notification_activated`, which is what lets a banner
/// select the session it is about.
///
/// Fire-and-forget by design: this is the *secondary* channel — the sidebar rail
/// survives it either way — so a failure must never surface as an error the
/// reader has to deal with.
export function notifyOS(sessionId: string, kind: NoticeKind, title: string, body: string) {
  // Remote, that command would post the banner on the machine holding the
  // runtime — a laptop in another room — and the reader holding the phone would
  // be told nothing at all. So the phone's own shell posts it, through the
  // notification plugin it carries for this one job. The cost, stated: the
  // plugin cannot report a click, so a phone banner says what happened and the
  // session it names is opened by hand.
  if (IS_REMOTE) return void notifyHere(title, body).catch(() => {});

  void call("notify_session", { sessionId, kind, title, body }).catch(() => {});
}

/// The plugin's own commands, invoked directly rather than through
/// `@tauri-apps/plugin-notification`.
///
/// The package is a thin wrapper over exactly these three calls, and adding it
/// to the desktop app's dependencies to reach them would put a plugin in the
/// bundle that only the mobile shell registers.
async function notifyHere(title: string, body: string) {
  const granted = await invoke<boolean | null>("plugin:notification|is_permission_granted");
  if (granted === false) {
    const answer = await invoke<string>("plugin:notification|request_permission");
    if (answer !== "granted") return;
  }

  await invoke("plugin:notification|notify", { options: { title, body } });
}

/// Posts one banner on the reader's own device, whichever device that is.
///
/// A phone cannot be checked against the laptop's notification centre, and this
/// channel is the one part of the app that says nothing at all when it is
/// broken — so there is a way to ask it for one.
export function notifyTest() {
  return IS_REMOTE
    ? notifyHere("Dray", "Notifications are working.")
    : call("notify_session", {
        sessionId: "",
        kind: "completed",
        title: "Dray",
        body: "Notifications are working.",
      });
}
