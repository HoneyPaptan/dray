import { useSyncExternalStore } from "react";
import { call } from "@/lib/transport";

import type { AgentAvailability, Harness } from "@/types/events";

/// Which agents have a CLI behind them on this machine.
///
/// A module store rather than per-hook state, for `useTheme`'s reason: the
/// picker and the composer's notice both read it, and two copies would let one
/// mark an agent unavailable while the other still offered to send to it.
///
/// Read once per process and never polled: a resolution is cached in Rust,
/// absence included, so refetching would spend a login shell to be told the
/// same thing. [`recheckAgents`] is the one thing that moves it, and it is a
/// button rather than a timer — only the reader knows they have just installed
/// something.
let answers: AgentAvailability[] | null = null;
let inFlight: Promise<void> | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (!answers && !inFlight) {
    inFlight = call<AgentAvailability[]>("agent_availability")
      .then((next) => {
        answers = next;
        emit();
      })
      // Silent, and deliberately: this decides whether to *warn*, so a failed
      // read must leave every agent offerable rather than mark them all
      // missing. The send-time check still refuses, with the same sentence.
      .catch(() => {})
      .finally(() => {
        inFlight = null;
      });
  }
  return () => {
    listeners.delete(listener);
  };
}

function snapshot() {
  return answers;
}

/// The agents, or `null` until the first read lands.
///
/// `null` is a resting state and not an error — the composer paints before the
/// read returns, so testing the list alone would flash a warning on an agent
/// that turns out to be installed.
export function useAgentAvailability(): AgentAvailability[] | null {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

/// Throws Rust's cached resolutions away and re-reads them.
///
/// The reader has just run an install command; without this the answer stays
/// whatever it was when the app started, which is what left the notice sitting
/// over a CLI that was by then installed.
///
/// Answers the fresh list so a caller can tell "installed now" from "still
/// missing" — the store's own emit cannot say which, since a notice that goes
/// away and one that stays both re-render. `null` where the read failed, which
/// is no news about the machine either way.
export async function recheckAgents(): Promise<AgentAvailability[] | null> {
  const next = await call<AgentAvailability[]>("recheck_agents").catch(() => null);
  if (!next) return null;
  answers = next;
  emit();
  return next;
}

/// What to say about one agent, or `null` where there is nothing to say —
/// either it is installed, or nobody has answered yet.
export function useMissingAgent(harness: Harness): AgentAvailability | null {
  const all = useAgentAvailability();
  const found = all?.find((a) => a.harness === harness);
  return found && !found.available ? found : null;
}
