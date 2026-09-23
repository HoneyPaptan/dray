import { useEffect, useSyncExternalStore } from "react";

import { call, subscribeEvent } from "@/lib/transport";

import type { FileBody, SaveOutcome } from "@/types/events";

/// A tagged union over the read, so there is no state in which a body exists
/// beside an error explaining why it doesn't.
///
/// `draft` is the buffer where it differs from disk and `null` where it does
/// not, so dirtiness is one null check. `stale` means the file moved on disk
/// under a dirty buffer: the disk copy is in `body`, the reader's in `draft`,
/// and nothing is written until they pick one.
export type FileState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | {
      status: "ready";
      body: FileBody;
      draft: string | null;
      saving: boolean;
      stale: boolean;
      saveError: string | null;
    };

const CLEAN = { draft: null, saving: false, stale: false, saveError: null } as const;

export type OpenFile = {
  /// Absolute. The tree opens files under the session's directory, and a chat
  /// link can open one from anywhere.
  path: string;
  /// The line a link named, where it named one. The viewer scrolls to it and
  /// marks it; nothing else reads it.
  line?: number;
  /// Bumped every time this path is opened afresh. What the viewer scrolls
  /// on — `line` alone cannot, since clicking the same link twice asks to be
  /// taken back to a line it is already sitting on.
  reveal: number;
  state: FileState;
};

type SessionFiles = { open: OpenFile[]; active: string | null };

/// Open files, per session, in the order they were opened.
///
/// Keyed by session for [useDocs](./useDocs.ts)'s reason: a file is opened from
/// one transcript or one session's tree, and a strip held for the whole app put
/// a tab opened in one session on screen in every other one. The view hides
/// rather than unmounts, so a session's own tabs survive being switched away
/// from and back either way.
///
/// In memory only, like `viewTabs` — a restart opens on an empty pane.
const bySession = new Map<string, SessionFiles>();

/// Shared by every session that has opened nothing, so an untouched session
/// costs no entry and keeps one snapshot identity across its renders.
const EMPTY: SessionFiles = { open: [], active: null };

/// Bumped every time a path is opened by a click, an already-open one included.
/// What tells `App` to bring the view forward — the count cannot, since
/// reopening an open file leaves it unchanged.
let opened = 0;

/// Bumped on every change. `useSyncExternalStore` subscribes to *this* rather
/// than to a built snapshot, so each caller's view is derived from the session
/// id it passed in.
let version = 0;

const listeners = new Set<() => void>();

function emit() {
  version += 1;
  for (const listener of listeners) listener();
}

/// Bumped whenever a read is issued for a path, and whenever the path is
/// closed. An async answer only lands where the number it captured is still
/// current, which stops a slow read writing over a newer one and stops it
/// re-adding a tab the reader has closed. Per path *and* per session, since two
/// sessions holding one file hold two reads.
const seqByPath = new Map<string, number>();

function issue(sid: string, path: string): number {
  const key = `${sid}\n${path}`;
  const next = (seqByPath.get(key) ?? 0) + 1;
  seqByPath.set(key, next);
  return next;
}

function current(sid: string, path: string, seq: number): boolean {
  return seqByPath.get(`${sid}\n${path}`) === seq;
}

function state(sid: string): SessionFiles {
  return bySession.get(sid) ?? EMPTY;
}

function write(sid: string, next: Partial<SessionFiles>) {
  bySession.set(sid, { ...state(sid), ...next });
  emit();
}

/// Replaces one file in place, leaving the rest and their identities alone. A
/// no-op where the path is gone, which is what the async guards come down to.
function patch(sid: string, path: string, next: (file: OpenFile) => OpenFile) {
  const { open } = state(sid);
  const at = open.findIndex((file) => file.path === path);
  if (at === -1) return;
  write(sid, { open: open.map((file, i) => (i === at ? next(file) : file)) });
}

/// Opens a file in the Files view, reading it if it is not already open.
///
/// An already-open path is activated and revealed rather than re-read: its text
/// is on screen and the file underneath is watched, so a second click is a
/// request to *look* at it, not to fetch it again.
export function openInFiles(sid: string | null, path: string, line?: number): void {
  // Nothing to open a file *into*. The view belongs to a session, and the one
  // route here with none selected is the issues page, which draws no view.
  if (!sid) return;

  opened += 1;
  const { open } = state(sid);

  if (open.some((file) => file.path === path)) {
    patch(sid, path, (file) => ({ ...file, line, reveal: file.reveal + 1 }));
    return write(sid, { active: path });
  }

  write(sid, {
    open: [...open, { path, line, reveal: 0, state: { status: "loading" } }],
    active: path,
  });
  read(sid, path);
}

/// Brings an already-open file forward.
export function activateFile(sid: string | null, path: string) {
  if (!sid) return;
  if (state(sid).active === path) return;
  write(sid, { active: path });
}

/// Closes a tab.
///
/// Closing the active one activates its left-hand neighbour, or the right-hand
/// one at the start — VS Code's rule, and the one that keeps `active` naming a
/// tab that is still open.
export function closeFile(sid: string | null, path: string) {
  if (!sid) return;
  const { open, active } = state(sid);
  const at = open.findIndex((file) => file.path === path);
  if (at === -1) return;
  const closing = open[at];
  if (closing.state.status === "ready" && closing.state.draft !== null) {
    // A stale buffer stays open: closing it would throw away the only copy of
    // the reader's edits with the strip that explains why still unanswered.
    if (closing.state.stale) return;
    void saveFile(sid, path);
  }
  // A read still out for this path must not write into the store after the
  // reader has closed it.
  issue(sid, path);
  const left = open.filter((file) => file.path !== path);
  write(sid, {
    open: left,
    active: active === path ? (left[at - 1]?.path ?? left[at]?.path ?? null) : active,
  });
}

/// Re-reads one open file, keeping it on screen until the new text lands.
///
/// Nothing is compared and nothing can be lost: this view never edits, so the
/// file on disk is always the right answer and adopting it silently is the
/// whole point — an agent rewriting what the reader is looking at should move
/// the text under them.
function read(sid: string, path: string) {
  const seq = issue(sid, path);
  call<FileBody>("read_file", { path })
    .then((body) => {
      if (!current(sid, path, seq)) return;
      patch(sid, path, (file) => ({ ...file, state: adopt(file.state, body) }));
    })
    .catch((err) => {
      if (!current(sid, path, seq)) return;
      patch(sid, path, (file) => ({
        ...file,
        state: { status: "error", message: String(err) },
      }));
    });
}

/// A fresh disk read lands over the old one, and a dirty buffer survives it. A
/// disk copy that now equals the draft means somebody saved the reader's own
/// words, so the draft is dropped; one that differs marks the buffer stale
/// rather than replacing what was typed.
function adopt(prev: FileState, body: FileBody): FileState {
  if (prev.status !== "ready" || prev.draft === null) {
    return { status: "ready", body, ...CLEAN };
  }
  const text = body.kind === "text" ? body.text : null;
  if (text === prev.draft) return { status: "ready", body, ...CLEAN };
  const moved = prev.body.kind !== "text" || text !== prev.body.text;
  return { ...prev, body, stale: prev.stale || moved };
}

function patchReady(
  sid: string,
  path: string,
  next: (state: Extract<FileState, { status: "ready" }>) => FileState,
) {
  patch(sid, path, (file) =>
    file.state.status === "ready" ? { ...file, state: next(file.state) } : file,
  );
}

function find(sid: string, path: string): OpenFile | undefined {
  return state(sid).open.find((file) => file.path === path);
}

export function isFileDirty(file: OpenFile): boolean {
  return file.state.status === "ready" && file.state.draft !== null;
}

export function editFile(sid: string | null, path: string, text: string) {
  if (!sid) return;
  patchReady(sid, path, (ready) => {
    if (ready.body.kind !== "text") return ready;
    const draft = text === ready.body.text ? null : text;
    return { ...ready, draft, stale: draft !== null && ready.stale };
  });
}

/// `expect` is the disk text the buffer was opened on, so a file that moved
/// answers `stale` with nothing written; `force` is the reader's own overwrite.
export async function saveFile(
  sid: string | null,
  path: string,
  { force = false }: { force?: boolean } = {},
): Promise<SaveOutcome | null> {
  if (!sid) return null;
  const file = find(sid, path);
  if (file?.state.status !== "ready" || file.state.body.kind !== "text") return null;
  const { draft, saving, stale, body } = file.state;
  if (draft === null || saving) return null;
  if (stale && !force) return "stale";

  const expect = force ? null : body.text;
  const seq = issue(sid, path);
  patchReady(sid, path, (ready) => ({ ...ready, saving: true, saveError: null }));

  try {
    const outcome = await call<SaveOutcome>("save_file", { path, text: draft, expect });
    if (!current(sid, path, seq)) return null;
    patchReady(sid, path, (ready) =>
      outcome === "stale"
        ? { ...ready, saving: false, stale: true }
        : { status: "ready", body: { kind: "text", text: draft }, ...CLEAN },
    );
    return outcome;
  } catch (err) {
    if (!current(sid, path, seq)) return null;
    patchReady(sid, path, (ready) => ({ ...ready, saving: false, saveError: String(err) }));
    return null;
  }
}

/// The blur save. Refuses a stale buffer, since writing over a file that moved
/// is the one thing autosave must never decide on its own.
export function autosaveFile(sid: string | null, path: string) {
  if (!sid) return;
  const file = find(sid, path);
  if (!file || file.state.status !== "ready" || file.state.draft === null) return;
  if (file.state.stale) return;
  void saveFile(sid, path);
}

export function saveActiveFile(sid: string | null) {
  if (!sid) return;
  const { active } = state(sid);
  if (!active) return;
  const file = find(sid, active);
  if (!file || !isFileDirty(file)) return;
  void saveFile(sid, active);
}

export function reloadFile(sid: string | null, path: string) {
  if (!sid) return;
  patchReady(sid, path, (ready) => ({ ...ready, ...CLEAN }));
  read(sid, path);
}

/// Re-reads every open file. What the turn ending and the header's refresh
/// call — the agent's writes are why this view needs either.
export function refreshOpenFiles(sid: string | null) {
  if (!sid) return;
  for (const file of state(sid).open) read(sid, file.path);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getVersion() {
  return version;
}

type FilesSnapshot = SessionFiles & { opened: number };

/// One session's view of the store. Exported so the split between sessions can
/// be read without a renderer.
export function openFilesFor(sid: string | null): FilesSnapshot {
  return { ...(sid ? state(sid) : EMPTY), opened };
}

/// A session's open files, and which one its viewer is showing.
export function useOpenFiles(sid: string | null): FilesSnapshot {
  useSyncExternalStore(subscribe, getVersion, getVersion);
  return openFilesFor(sid);
}

/// Which watcher this view owns. The Docs panel holds `"docs"`, and a single
/// watcher for both would mean whichever opened last silently took the other's
/// watch away.
const SCOPE = "files";

/// Every `watch_docs` call for this scope, in the order it was made — the same
/// chain [useDocWatcher](./useDocs.ts) keeps, and for the same reason: the
/// command replaces the whole set for a scope, and two `call`s can land in
/// either order. A failed call is swallowed so one cannot break the chain for
/// the rest of the run.
let watching: Promise<unknown> = Promise.resolve();

function watch(paths: string[]): Promise<unknown> {
  watching = watching
    .then(() => call("watch_docs", { scope: SCOPE, paths }))
    .catch(() => {});
  return watching;
}

/// Keeps the open files current with the files underneath them.
export function useOpenFilesWatcher(sid: string | null) {
  const { open } = useOpenFiles(sid);
  // A joined key rather than the array, so a render that rebuilds the list
  // without changing the set does not re-arm the watcher. Only ever a
  // dependency — the paths sent are read off the store, never split back out of
  // this, since a newline is legal in a path.
  const key = open.map((file) => file.path).join("\n");

  useEffect(() => {
    void watch(openFilesFor(sid).open.map((file) => file.path));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, sid]);

  // The view unmounts when no session is selected, and a watcher left armed
  // there emits into nothing for the rest of the run.
  useEffect(() => () => void watch([]), []);

  // Re-registered per session rather than held with `[]` deps and a ref: the
  // watch set is this session's files, so an event arriving under a listener
  // still closed over the last one would re-read a path that session may not
  // even have open.
  useEffect(() => {
    const un = subscribeEvent<string>("doc_changed", (event) => {
      if (!sid) return;
      if (state(sid).open.some((file) => file.path === event.payload)) {
        read(sid, event.payload);
      }
    });
    return () => void un.then((off) => off());
  }, [sid]);
}
