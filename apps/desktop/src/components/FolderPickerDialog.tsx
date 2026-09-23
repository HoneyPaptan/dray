import { useEffect, useState } from "react";
import { ChevronUp, Folder, GitBranch } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { resolveFolder, useFolderPickerOpen } from "@/lib/folderPicker";
import { call } from "@/lib/transport";
import type { FolderListing } from "@/types/events";

/// Picks a directory on the machine the agent runs on.
///
/// The phone has no native folder picker and would pick the wrong machine's
/// folder if it had one, so this browses the desktop over the transport. One
/// dialog for the app, fed by a module store, since the caller is an async
/// handler in `useSessions` with nowhere to draw.
///
/// Read-only and deliberately plain: no create, no rename, no favourites. It
/// answers one question, and every other thing a file manager does is a thing
/// the reader has a file manager for.
export default function FolderPickerDialog() {
  const open = useFolderPickerOpen();
  const [listing, setListing] = useState<FolderListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Not derived from `listing`: a step that fails has to leave the previous
  // listing on screen, so what is being loaded and what is drawn differ.
  const [loading, setLoading] = useState(false);

  // An empty path is home, which is also the only way the phone learns where
  // home is. Re-read on every open rather than cached: the tree is on another
  // machine and may have moved since it was last looked at.
  useEffect(() => {
    if (!open) return;
    setListing(null);
    setError(null);
    void step("");
  }, [open]);

  async function step(path: string) {
    setLoading(true);
    try {
      setListing(await call<FolderListing>("list_folders", { path }));
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(next) => !next && resolveFolder(null)}>
      <DialogContent className="flex h-[32rem] max-h-[80vh] flex-col gap-0 sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Attach project</DialogTitle>
          {/* The path is the description, since on a phone it is the only
              thing saying which machine this is walking. Broken anywhere, or a
              deep path pushes the dialog wider than the screen. */}
          <DialogDescription className="break-all font-mono text-ui">
            {listing?.path ?? "…"}
          </DialogDescription>
        </DialogHeader>

        <div className="-mx-6 mt-2 min-h-0 flex-1 overflow-y-auto px-4">
          {listing?.parent != null && (
            <button
              type="button"
              onClick={() => void step(listing.parent as string)}
              className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-ui hover:bg-accent"
            >
              <ChevronUp className="size-4 shrink-0 text-muted-foreground" />
              <span className="truncate text-muted-foreground">Up</span>
            </button>
          )}

          {listing?.entries.map((entry) => (
            <button
              key={entry.path}
              type="button"
              onClick={() => void step(entry.path)}
              className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-ui hover:bg-accent"
            >
              <Folder className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 truncate">{entry.name}</span>
              {/* The row almost everybody is looking for, marked in the muted
                  colour the model picker's own qualifiers take: presence is
                  the message, and a repo is not a warning. */}
              {entry.isRepo && (
                <GitBranch className="ml-auto size-3.5 shrink-0 text-muted-foreground/60" />
              )}
            </button>
          ))}

          {/* Three states, one slot, and they are genuinely different: a
              folder holding no subfolders is an ordinary place to stand and
              still the place you may want to attach. */}
          {error ? (
            <p className="px-2 py-3 text-destructive text-ui">{error}</p>
          ) : loading && !listing ? (
            <p className="px-2 py-3 text-muted-foreground text-ui">Reading…</p>
          ) : listing && listing.entries.length === 0 ? (
            <p className="px-2 py-3 text-muted-foreground text-ui">No folders in here.</p>
          ) : null}
        </div>

        <div className="mt-2 flex justify-end gap-2 border-border border-t pt-4">
          <Button type="button" variant="ghost" onClick={() => resolveFolder(null)}>
            Cancel
          </Button>
          {/* The folder being *stood in* is what gets attached, not a row that
              was tapped — tapping a row steps into it, so a pick would need a
              second gesture the phone does not have. */}
          <Button
            type="button"
            disabled={!listing}
            onClick={() => listing && resolveFolder(listing.path)}
          >
            Attach this folder
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
