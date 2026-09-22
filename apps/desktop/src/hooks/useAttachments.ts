import { useCallback, useSyncExternalStore } from "react";
import { call, IS_REMOTE } from "@/lib/transport";
import { open } from "@tauri-apps/plugin-dialog";

import type { Attachment } from "@/types/events";

/// What is pinned to the composer but not yet sent, keyed by the session it was
/// attached to. `null` is the new task's own key, exactly as in `useDraft` — and
/// for the same reason: `AppShell` moves the footer when it is centered, so
/// crossing from the empty state into a session unmounts `ChatInput` and mounts
/// a fresh one. Anything held in component state would be lost on that switch.
///
/// Module-level also because there are two writers in two places. The `+` button
/// lives in `ComposerToolbar`, which is passed to `ChatInput` as an opaque
/// `ReactNode`, so the two cannot pass props to each other — they share this
/// instead, and neither has to know the other exists.
///
/// Not persisted: an attachment is part of a sentence you were in the middle of,
/// and the file it points at may not survive a restart either.
const bySession = new Map<string | null, Attachment[]>();
const listeners = new Set<() => void>();

// One frozen array for every empty key. `useSyncExternalStore` re-renders on any
// snapshot that isn't reference-equal to the last, so minting `[]` per read
// would loop forever.
const EMPTY: Attachment[] = [];

function emit() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function write(sessionId: string | null, next: Attachment[]) {
  if (next.length) bySession.set(sessionId, next);
  else bySession.delete(sessionId);
  emit();
}

/// Describes each path in the backend and pins the ones that can be attached.
/// Deduped on path, so dropping the same screenshot twice pins one — the path is
/// the identity, and a second copy of one file says nothing the first didn't.
export async function addAttachmentPaths(sessionId: string | null, paths: string[]) {
  const current = bySession.get(sessionId) ?? EMPTY;
  const fresh = paths.filter((path) => !current.some((a) => a.path === path));
  if (!fresh.length) return;

  const added = await call<Attachment[]>("read_attachments", { paths: fresh });
  if (!added.length) return;

  // Re-read rather than closing over `current`: the dialog and the reads above
  // are both awaited, and a drop landing in between must not be dropped.
  const now = bySession.get(sessionId) ?? EMPTY;
  write(sessionId, [...now, ...added.filter((a) => !now.some((b) => b.path === a.path))]);
}

/// Opens the system file picker and pins whatever comes back. Resolves to
/// nothing when the user cancels.
export async function pickAttachments(sessionId: string | null) {
  // A remote client picks files on its own device, and every command runs on the
  // machine holding the runtime — so the path it picked names nothing there and
  // describing it answered an empty list. The bytes are the only thing that can
  // cross, so the file goes up once and the backend hands back an ordinary
  // attachment pointing at a file it now holds.
  if (IS_REMOTE) return uploadPicked(sessionId);

  const picked = await open({ multiple: true, title: "Attach files" });
  if (!picked) return;

  await addAttachmentPaths(sessionId, Array.isArray(picked) ? picked : [picked]);
}

/// The webview's own picker, which is the one that hands back bytes.
///
/// `plugin-dialog` answers with a path, which is exactly what a remote client
/// cannot use. A plain file input is also what reaches Android's photo picker
/// and its camera, both of which the shell would otherwise need permissions and
/// a plugin apiece to offer.
function pickFiles(): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.className = "hidden";
    document.body.appendChild(input);

    const done = (files: File[]) => {
      input.remove();
      resolve(files);
    };

    input.addEventListener("change", () => done([...(input.files ?? [])]), { once: true });
    // Chrome fires this on a dismissed picker; without it a cancel would leave
    // the promise pending for the life of the page.
    input.addEventListener("cancel", () => done([]), { once: true });
    input.click();
  });
}

/// Base64 without reading the whole file into a string a character at a time.
/// `FileReader` hands back a `data:` URL, whose payload is already the encoding
/// we want — spreading a multi-megabyte `Uint8Array` into `fromCharCode` blows
/// the argument limit instead.
function encode(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error);
    reader.onload = () => {
      const url = String(reader.result);
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.readAsDataURL(file);
  });
}

async function uploadPicked(sessionId: string | null) {
  const files = await pickFiles();
  if (!files.length) return;

  for (const file of files) {
    try {
      const added = await call<Attachment>("upload_attachment", {
        name: file.name,
        data: await encode(file),
      });
      // Re-read per file rather than once at the end: an upload is a round trip
      // and the reader may pin something else while it is in flight.
      const now = bySession.get(sessionId) ?? EMPTY;
      if (!now.some((a) => a.path === added.path)) write(sessionId, [...now, added]);
    } catch (e) {
      console.error(`could not attach ${file.name}`, e);
    }
  }
}

export function removeAttachment(sessionId: string | null, path: string) {
  const current = bySession.get(sessionId);
  if (!current) return;

  write(
    sessionId,
    current.filter((a) => a.path !== path),
  );
}

export function clearAttachments(sessionId: string | null) {
  if (!bySession.has(sessionId)) return;
  write(sessionId, EMPTY);
}

/// One session's pending attachments. Read-only — every mutation is a
/// module-level function above, so a caller that only writes (the toolbar's `+`)
/// takes no subscription and re-renders for nothing.
export function useAttachments(sessionId: string | null): Attachment[] {
  const getSnapshot = useCallback(() => bySession.get(sessionId) ?? EMPTY, [sessionId]);

  return useSyncExternalStore(subscribe, getSnapshot);
}

/// Pins attachments already described — what a cancelled prompt hands back.
/// Synchronous, unlike `addAttachmentPaths`, since these need no describing.
/// Appended and deduped like a drop, because whatever is pinned now is the
/// user's too.
export function restoreAttachments(sessionId: string | null, attachments: Attachment[]) {
  if (!attachments.length) return;

  const current = bySession.get(sessionId) ?? EMPTY;
  const fresh = attachments.filter((a) => !current.some((b) => b.path === a.path));
  if (!fresh.length) return;

  write(sessionId, [...current, ...fresh]);
}
