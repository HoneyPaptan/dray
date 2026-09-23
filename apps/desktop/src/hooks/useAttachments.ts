import { useCallback, useSyncExternalStore } from "react";
import { call, IS_REMOTE } from "@/lib/transport";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";

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

/// Why the last attach did nothing, or `null`.
///
/// An upload is the one attach path that can fail for a reason the reader has
/// to act on — a phone's bytes cross a socket and a relay, and a short read
/// there is refused in Rust rather than written. That used to reach a
/// `console.error` and nothing else, so the file simply never appeared and the
/// tray said as much about it as about a file nobody picked. Module-level for
/// the same reason the attachments themselves are: the `+` button and the
/// composer cannot pass props to each other.
let attachError: string | null = null;

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

/// The last segment of a picked location, for the upload's name.
///
/// On Android the dialog answers a `content://` URI whose tail is a bare id
/// rather than a filename; the backend names the bytes by their own magic where
/// the name carries no extension, so a bare id is enough here.
function pickedName(location: string): string {
  const tail = location.split("/").filter(Boolean).pop() ?? "upload";
  try {
    return decodeURIComponent(tail);
  } catch {
    return tail;
  }
}

/// The phone's picker, through the shell rather than the WebView.
///
/// `<input type=file>` was the first shape and its blobs arrived short — five
/// screenshots reached the laptop as valid, padded base64 of a truncated JPEG,
/// so the bytes were already cut inside the WebView, and re-reading the same
/// blob re-read the same cut. The dialog plugin answers a `content://` URI and
/// the fs plugin opens it through the ContentResolver from Rust, which is a
/// real file descriptor read to its end rather than whatever the WebView's
/// blob layer handed over.
///
/// `null` where the shell offers no dialog — a browser tab on the mobile build —
/// so the caller can fall back to the input, which is where this started.
async function pickThroughShell(): Promise<{ name: string; bytes: Uint8Array }[] | null> {
  let picked: string | string[] | null;
  try {
    picked = await open({ multiple: true, title: "Attach files" });
  } catch {
    return null;
  }
  if (!picked) return [];

  const locations = Array.isArray(picked) ? picked : [picked];
  const files: { name: string; bytes: Uint8Array }[] = [];
  for (const location of locations) {
    files.push({ name: pickedName(location), bytes: await readFile(location) });
  }
  return files;
}

/// The webview's own picker, kept as the fallback for a mobile build running
/// somewhere the shell's dialog is not — a browser tab under `dev:mobile`.
/// On the phone itself `pickThroughShell` is what runs, since this one's blobs
/// arrived short.
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

/// How many times a short read is tried again before it is given up on.
///
/// Three because a re-read is cheap and the failure it cures is intermittent:
/// the same screenshot that arrives short once arrives whole on the next pass.
const READ_ATTEMPTS = 3;

/// Every byte of `file`, or a refusal naming how much of it arrived.
///
/// A phone's file comes out of a content provider, not off a path, and that
/// stream can end early — Android hands the blob back with no error and the
/// read resolves with a prefix. Measured: five screenshots attached from the
/// phone reached the runtime as valid base64 of a *truncated* JPEG, so the
/// bytes were already short in the webview rather than cut anywhere on the
/// wire.
///
/// `file.size` is the provider's own answer and is what a short read is judged
/// against. A provider that understates it defeats this, which is why
/// `upload_attachment` still checks the picture's own terminator on the far
/// side — this is the half that can retry, that one is the half that cannot be
/// fooled.
async function readWhole(file: File): Promise<Uint8Array> {
  let shortest = 0;

  for (let attempt = 0; attempt < READ_ATTEMPTS; attempt++) {
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (!file.size || bytes.byteLength >= file.size) return bytes;
    shortest = bytes.byteLength;
  }

  throw new Error(
    `${file.name} could not be read whole (${shortest} of ${file.size} bytes). Try attaching it again.`,
  );
}

/// Base64 without spreading a multi-megabyte array into one call.
/// `String.fromCharCode` takes its bytes as arguments, so a whole image at once
/// blows the argument limit; 32KB at a time is well under it.
function toBase64(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

async function uploadPicked(sessionId: string | null) {
  attachError = null;
  emit();

  let files: { name: string; bytes: () => Promise<Uint8Array> }[];
  try {
    const shell = await pickThroughShell();
    files = shell
      ? shell.map((f) => ({ name: f.name, bytes: async () => f.bytes }))
      : (await pickFiles()).map((f) => ({ name: f.name, bytes: () => readWhole(f) }));
  } catch (e) {
    attachError = e instanceof Error ? e.message : "could not read the picked file";
    emit();
    return;
  }
  if (!files.length) return;

  for (const file of files) {
    try {
      const added = await call<Attachment>("upload_attachment", {
        name: file.name,
        data: toBase64(await file.bytes()),
      });
      // Re-read per file rather than once at the end: an upload is a round trip
      // and the reader may pin something else while it is in flight.
      const now = bySession.get(sessionId) ?? EMPTY;
      if (!now.some((a) => a.path === added.path)) write(sessionId, [...now, added]);
    } catch (e) {
      console.error(`could not attach ${file.name}`, e);
      attachError = e instanceof Error ? e.message : `could not attach ${file.name}`;
      emit();
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

/// The last attach failure, cleared when the next attach starts.
export function useAttachError(): string | null {
  return useSyncExternalStore(subscribe, () => attachError);
}
