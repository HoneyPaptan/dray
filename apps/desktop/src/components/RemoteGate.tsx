import { useCallback, useState, useSyncExternalStore } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { readEndpoint, remote, setEndpoint, type RemoteStatus } from "@/lib/remoteTransport";
import { IS_REMOTE } from "@/lib/transport";

const DEFAULT_PORT = 8787;

/// The phone's own connection state, read the way every other store here is.
function useRemoteStatus(): RemoteStatus {
  return useSyncExternalStore(
    (notify) => remote.watchStatus(notify),
    () => remote.status,
    () => "unconfigured" as const,
  );
}

/**
 * Stands between the phone and the app until a host is configured.
 *
 * Children stay mounted through a dropped connection — a phone loses its socket
 * every time the screen goes off, and unmounting the app there would throw away
 * every loaded transcript for a gap the transport reconnects on its own.
 */
export function RemoteGate({ children }: { children: React.ReactNode }) {
  const status = useRemoteStatus();
  if (!IS_REMOTE) return <>{children}</>;
  if (status === "unconfigured" && !readEndpoint()) return <ConnectForm />;
  return (
    <>
      {children}
      {status !== "open" && <ConnectionNotice status={status} />}
    </>
  );
}

function ConnectForm() {
  const [host, setHost] = useState("");
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);

  const connect = useCallback(() => {
    const url = normalise(host);
    if (!url) return setError("That does not look like an address");
    if (!token.trim()) return setError("The host's token is what opens it");
    setEndpoint({ url, token: token.trim() });
  }, [host, token]);

  return (
    <div className="flex h-dvh w-full items-center justify-center p-6">
      <div className="flex w-full max-w-sm flex-col gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-lg font-medium">Connect to your Dray</h1>
          <p className="text-muted-foreground text-sm">
            Everything runs on your laptop. This is the address it answers on, and the token
            it minted for itself.
          </p>
        </div>
        <Input
          autoFocus
          inputMode="url"
          placeholder="100.101.14.98"
          value={host}
          onChange={(e) => {
            setHost(e.target.value);
            setError(null);
          }}
        />
        <Input
          placeholder="Token"
          value={token}
          onChange={(e) => {
            setToken(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => e.key === "Enter" && connect()}
        />
        {error && <p className="text-destructive text-sm">{error}</p>}
        <Button onClick={connect}>Connect</Button>
        <p className="text-muted-foreground text-xs">
          The token is in <code>~/.dray/remote-token</code> on the laptop.
        </p>
      </div>
    </div>
  );
}

function ConnectionNotice({ status }: { status: RemoteStatus }) {
  const word = status === "connecting" ? "Connecting" : "Reconnecting";
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex justify-center p-3">
      <div className="bg-card text-muted-foreground pointer-events-auto rounded-full border px-3 py-1 text-xs shadow">
        {word} to your Dray
        <button className="ml-2 underline" onClick={() => setEndpoint(null)}>
          Change host
        </button>
      </div>
    </div>
  );
}

/// A bare address is the common case on a tailnet, so it grows the scheme and
/// the default port rather than being refused.
function normalise(raw: string): string | null {
  const trimmed = raw.trim().replace(/\/$/, "");
  if (!trimmed) return null;
  if (/^wss?:\/\//.test(trimmed)) return trimmed;
  const withPort = /:\d+$/.test(trimmed) ? trimmed : `${trimmed}:${DEFAULT_PORT}`;
  return `ws://${withPort}`;
}
