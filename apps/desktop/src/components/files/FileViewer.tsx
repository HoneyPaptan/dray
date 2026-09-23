import CodeEditor from "@/components/files/CodeEditor";
import { Button } from "@/components/ui/button";
import {
  autosaveFile,
  editFile,
  reloadFile,
  saveFile,
  type OpenFile,
} from "@/hooks/useOpenFiles";
import { cn } from "@/lib/utils";

export default function FileViewer({
  sessionId,
  file,
  locked,
}: {
  sessionId: string;
  file: OpenFile | null;
  /// True while the session's agent is mid-turn. The buffer goes read-only, or
  /// the reader and the agent write the same file and the last one wins.
  locked: boolean;
}) {
  if (!file) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center px-6 text-ui text-muted-foreground">
        Pick a file to read it.
      </div>
    );
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <Strip sessionId={sessionId} file={file} locked={locked} />
      <Body sessionId={sessionId} file={file} locked={locked} />
    </div>
  );
}

/// Only drawn where something needs settling: the file moved on disk under an
/// unsaved buffer, a save failed, or the agent holds the file for now.
function Strip({
  sessionId,
  file,
  locked,
}: {
  sessionId: string;
  file: OpenFile;
  locked: boolean;
}) {
  if (file.state.status !== "ready") return null;
  const { stale, saveError, draft } = file.state;
  const lockedNote = locked && file.state.body.kind === "text";
  if (!stale && !saveError && !lockedNote) return null;

  return (
    <div className="shrink-0 space-y-1.5 border-b border-border px-3 py-2 text-ui">
      {stale && (
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 text-muted-foreground">
            This file changed on disk since you opened it.
          </span>
          <Button size="sm" variant="outline" onClick={() => reloadFile(sessionId, file.path)}>
            Discard my edits
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void saveFile(sessionId, file.path, { force: true })}
          >
            Overwrite
          </Button>
        </div>
      )}
      {saveError && <p className="text-destructive">{saveError}</p>}
      {lockedNote && !stale && (
        <p className="text-muted-foreground">
          {draft !== null
            ? "Read only while the agent is working. Your unsaved edits are kept."
            : "Read only while the agent is working."}
        </p>
      )}
    </div>
  );
}

function Body({
  sessionId,
  file,
  locked,
}: {
  sessionId: string;
  file: OpenFile;
  locked: boolean;
}) {
  const note = (text: string, tone?: "error") => (
    <p
      className={cn(
        "px-3 py-2 text-ui",
        tone === "error" ? "text-destructive" : "text-muted-foreground",
      )}
    >
      {text}
    </p>
  );

  if (file.state.status === "loading") return note("Loading…");
  if (file.state.status === "error") return note(file.state.message, "error");

  if (file.state.body.kind === "image") {
    return (
      <div
        className="flex min-h-full items-center justify-center p-6"
        style={{
          backgroundImage:
            "repeating-conic-gradient(var(--muted) 0% 25%, transparent 0% 50%)",
          backgroundSize: "16px 16px",
        }}
      >
        <img
          src={file.state.body.dataUrl}
          alt={file.path}
          className="max-h-full max-w-full object-contain"
        />
      </div>
    );
  }

  const value = file.state.draft ?? file.state.body.text;

  return (
    <CodeEditor
      key={file.path}
      path={file.path}
      value={value}
      readOnly={locked}
      line={file.line}
      reveal={file.reveal}
      onChange={(text) => editFile(sessionId, file.path, text)}
      onBlur={() => autosaveFile(sessionId, file.path)}
    />
  );
}
