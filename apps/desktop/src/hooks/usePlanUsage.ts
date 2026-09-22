import { useEffect, useState } from "react";

import { call } from "@/lib/transport";
import type { Harness, PlanWindow } from "@/types/events";

/// What the account has left of its plan, read when the picker opens.
///
/// Gated on `enabled` rather than fetched with the session: the answer costs a
/// throwaway CLI process, and the only place it is drawn is the foot of a menu
/// most turns never open. The backend caches for a minute, so opening the menu
/// repeatedly costs one probe.
///
/// A failed read is an empty list, never an error on screen — the composer's
/// error slot is for what the reader just did, and they opened a menu.
export function usePlanUsage(
  harness: Harness,
  cwd: string | null,
  enabled: boolean,
): PlanWindow[] {
  const [windows, setWindows] = useState<PlanWindow[]>([]);

  useEffect(() => {
    if (!enabled) return;

    // The read outlives the menu it was opened for, and the harness can change
    // under it — so a landing answer is dropped unless it is still the one
    // asked for, the same guard every keyed read in this app makes.
    let live = true;
    void call<PlanWindow[]>("plan_usage", { cwd, harness })
      .then((next) => live && setWindows(next))
      .catch(() => live && setWindows([]));

    return () => {
      live = false;
    };
  }, [harness, cwd, enabled]);

  return windows;
}
