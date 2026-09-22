import { describe, expect, it } from "vitest";

import { reconcileStatuses } from "./liveStatus";
import type { SessionIndexItem, SessionStatus } from "@/types/events";

const index = (entries: [string, SessionStatus][]): SessionIndexItem[] =>
  entries.map(([sessionId, status]) => ({ sessionId, status }) as SessionIndexItem);

describe("reconcileStatuses", () => {
  /// The bug this exists for: a turn ends while the phone is asleep, the
  /// `session_status` that said so is never replayed, and the composer draws a
  /// Stop button over a session that finished ten minutes ago.
  it("clears an in_progress the backend does not confirm", () => {
    const next = reconcileStatuses({ a: "in_progress" }, {}, index([["a", "completed"]]));

    expect(next.a).toBe("completed");
  });

  /// A session with no child and nothing persisted is simply idle.
  it("falls to idle where neither source knows the session", () => {
    expect(reconcileStatuses({ a: "in_progress" }, {}, []).a).toBe("idle");
  });

  /// A turn genuinely still running must survive a reconnection, or every
  /// reconnect would stop the working indicator mid-turn.
  it("leaves a confirmed in_progress alone", () => {
    const previous: Record<string, SessionStatus> = { a: "in_progress" };

    expect(reconcileStatuses(previous, { a: "in_progress" }, [])).toBe(previous);
  });

  /// `completed` is a reader-facing fact the backend does not keep — it means
  /// "finished and unread" — so correcting against a live map that has never
  /// heard of it would mark every unread session read.
  it("never touches a status that is not in_progress", () => {
    const previous: Record<string, SessionStatus> = { a: "completed", b: "idle" };

    expect(reconcileStatuses(previous, {}, [])).toBe(previous);
  });

  /// Identity where nothing moved, so a healthy reconnection re-renders
  /// nothing.
  it("answers the same object when there is nothing to correct", () => {
    const previous: Record<string, SessionStatus> = { a: "in_progress" };

    expect(reconcileStatuses(previous, { a: "in_progress" }, [])).toBe(previous);
  });
});
