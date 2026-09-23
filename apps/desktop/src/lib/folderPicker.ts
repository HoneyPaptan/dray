import { useSyncExternalStore } from "react";

/// The promise a `pickFolder` caller is waiting on, or `null` when the dialog
/// is shut.
let pending: ((path: string | null) => void) | null = null;
const listeners = new Set<() => void>();

function set(resolve: ((path: string | null) => void) | null) {
  pending = resolve;
  for (const l of listeners) l();
}

/// Whether the folder picker is up. `App` mounts one dialog and reads this,
/// the same bargain [`usePendingLink`](./openLink.ts) makes: the caller is an
/// async handler several components away from anything that can draw.
export function useFolderPickerOpen(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    () => pending !== null,
  );
}

/// Asks the reader for a directory **on the machine the agent runs on**, and
/// resolves with its absolute path or `null` where they backed out.
///
/// The native dialog cannot serve the phone for two reasons, and fixing the
/// first leaves the second: Android's dialog plugin implements no directory
/// picker, and a picker there would in any case choose a folder on the phone,
/// where a project has to be a directory on the desktop. So this browses the
/// desktop over the transport instead.
///
/// A second call while one is open answers the first with `null` rather than
/// leaving it hanging — the dialog is one, so the promise behind it has to be.
export function pickFolder(): Promise<string | null> {
  pending?.(null);
  return new Promise((resolve) => set(resolve));
}

export function resolveFolder(path: string | null) {
  const resolve = pending;
  set(null);
  resolve?.(path);
}
