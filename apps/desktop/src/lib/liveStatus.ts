import type { SessionIndexItem, SessionStatus } from "@/types/events";

/// The status map corrected against what the backend says is actually running.
///
/// `session_status` is pushed and never replayed, so a frontend that was not
/// listening when a turn ended cannot learn that it did — and this map outranks
/// the index everywhere it is read, so no refetch corrects it either. On the
/// desktop that never bites, the frontend and the manager sharing a process. On
/// the phone it bites constantly: a sleeping screen, a network move or the host
/// restarting drops the socket, and a turn ending while it is down leaves the
/// session drawing "running" with a Stop button under it for good.
///
/// **Only `in_progress` is corrected, and only downward.** Everything else here
/// is a reader-facing fact the backend does not keep — `completed` means
/// "finished and unread", cleared by looking at the row — so replacing the map
/// wholesale would mark every unread session read.
///
/// A session the backend does not list has no child at all, so it is not
/// running whatever this map remembers. Its own persisted status stands in
/// rather than a flat `idle`: a turn that finished while the socket was down is
/// unread, not nothing.
export function reconcileStatuses(
  previous: Record<string, SessionStatus>,
  live: Record<string, SessionStatus>,
  index: SessionIndexItem[],
): Record<string, SessionStatus> {
  let changed = false;
  const next = { ...previous };

  for (const [sessionId, status] of Object.entries(previous)) {
    if (status !== "in_progress" || live[sessionId] === "in_progress") continue;

    next[sessionId] =
      live[sessionId]
      ?? index.find((item) => item.sessionId === sessionId)?.status
      ?? "idle";
    changed = true;
  }

  // The same object where nothing moved, so a reconnection that found
  // everything in order re-renders nothing.
  return changed ? next : previous;
}
