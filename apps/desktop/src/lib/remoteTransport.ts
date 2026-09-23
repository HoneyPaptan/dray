import type { UnlistenFn } from "@tauri-apps/api/event";

/** Where a remote runtime is reached, and the token that opens it. */
export type RemoteEndpoint = { url: string; token: string };

const ENDPOINT_KEY = "dray.remote";

/// The endpoint the build was given, for a phone that has never been pointed
/// at one by hand.
///
/// `scripts/mobile-env.mjs` writes these before `build:mobile` from the three
/// things only this machine knows: its tailnet address, the port the desktop
/// serves on, and the token the desktop minted for itself. So installing the
/// APK is the whole of pairing.
///
/// Read **below** the stored endpoint, never over it: the connect form is still
/// how a phone is moved to another host, and a baked default that outranked it
/// would undo that on every launch. Absent on the desktop build, and absent on
/// a mobile build made without the script, where the form is the only route in
/// exactly as before.
const BAKED: RemoteEndpoint | null =
  import.meta.env.VITE_DRAY_URL && import.meta.env.VITE_DRAY_TOKEN
    ? { url: import.meta.env.VITE_DRAY_URL, token: import.meta.env.VITE_DRAY_TOKEN }
    : null;

const RECONNECT_MIN_MS = 500;
const RECONNECT_MAX_MS = 15_000;

type Pending = {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
};

type ServerFrame =
  | { id: number; ok: true; value: unknown }
  | { id: number; ok: false; error: string }
  | { event: string; payload: unknown };

/** Read the stored endpoint, or null where the app has not been pointed at one yet. */
export function readEndpoint(): RemoteEndpoint | null {
  try {
    const raw = localStorage.getItem(ENDPOINT_KEY);
    if (!raw) return BAKED;
    const parsed = JSON.parse(raw) as Partial<RemoteEndpoint>;
    if (!parsed.url || !parsed.token) return BAKED;
    return { url: parsed.url, token: parsed.token };
  } catch {
    return BAKED;
  }
}

/** Store the endpoint and reconnect to it. */
export function setEndpoint(endpoint: RemoteEndpoint | null) {
  if (endpoint) localStorage.setItem(ENDPOINT_KEY, JSON.stringify(endpoint));
  else localStorage.removeItem(ENDPOINT_KEY);
  remote.reconnect();
}

/** Connection state, for the shell to draw while the socket is down. */
export type RemoteStatus = "unconfigured" | "connecting" | "open" | "closed";

class RemoteTransport {
  #socket: WebSocket | null = null;
  #nextId = 1;
  #pending = new Map<number, Pending>();
  #handlers = new Map<string, Set<(event: { payload: unknown }) => void>>();
  #status: RemoteStatus = "unconfigured";
  #statusWatchers = new Set<(status: RemoteStatus) => void>();
  #backoff = RECONNECT_MIN_MS;
  #retry: ReturnType<typeof setTimeout> | null = null;
  #queue: string[] = [];
  /// What image URLs are signed with, handed over once the socket has
  /// authenticated. Never the endpoint's own token — that opens the socket,
  /// which can create sessions and send prompts, and a URL ends up in the DOM.
  #assetTicket: string | null = null;

  get status(): RemoteStatus {
    return this.#status;
  }

  /** Subscribe to connection state. Returns the unsubscribe function. */
  watchStatus(watcher: (status: RemoteStatus) => void): () => void {
    this.#statusWatchers.add(watcher);
    return () => this.#statusWatchers.delete(watcher);
  }

  #setStatus(status: RemoteStatus) {
    if (this.#status === status) return;
    this.#status = status;
    for (const watcher of this.#statusWatchers) watcher(status);
  }

  /** Drop the current socket and open a fresh one against the stored endpoint. */
  reconnect() {
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = null;
    this.#backoff = RECONNECT_MIN_MS;
    const socket = this.#socket;
    this.#socket = null;
    socket?.close();
    this.#open();
  }

  #open() {
    const endpoint = readEndpoint();
    if (!endpoint) {
      this.#setStatus("unconfigured");
      return;
    }
    if (this.#socket) return;

    this.#setStatus("connecting");
    const url = endpoint.url.replace(/\/$/, "");
    const socket = new WebSocket(url);
    this.#socket = socket;

    socket.onopen = () => {
      this.#backoff = RECONNECT_MIN_MS;
      // The token is the first frame rather than a header: a browser cannot put
      // one on a websocket, and the handshake's query string is gone by the
      // time the server is reading frames. Ordering is what makes it safe — the
      // server refuses everything until it has seen this.
      socket.send(JSON.stringify({ token: endpoint.token }));
      this.#setStatus("open");
      for (const frame of this.#queue.splice(0)) socket.send(frame);
    };
    socket.onmessage = (message) => this.#receive(message.data as string);
    socket.onclose = () => {
      if (this.#socket !== socket) return;
      this.#socket = null;
      this.#assetTicket = null;
      this.#setStatus("closed");
      // Every outstanding call is unanswerable now; leaving them pending would
      // hang the caller for the life of the app.
      for (const pending of this.#pending.values()) {
        pending.reject(new Error("connection to the Dray host was lost"));
      }
      this.#pending.clear();
      this.#scheduleRetry();
    };
    socket.onerror = () => socket.close();
  }

  #scheduleRetry() {
    if (this.#retry) return;
    const delay = this.#backoff;
    this.#backoff = Math.min(this.#backoff * 2, RECONNECT_MAX_MS);
    this.#retry = setTimeout(() => {
      this.#retry = null;
      this.#open();
    }, delay);
  }

  #receive(raw: string) {
    let frame: ServerFrame;
    try {
      frame = JSON.parse(raw) as ServerFrame;
    } catch {
      return;
    }
    if ("event" in frame) {
      if (frame.event === "remote_ready") {
        this.#assetTicket = (frame.payload as { asset?: string } | null)?.asset ?? null;
        return;
      }
      const handlers = this.#handlers.get(frame.event);
      if (handlers) for (const handler of [...handlers]) handler({ payload: frame.payload });
      return;
    }
    const pending = this.#pending.get(frame.id);
    if (!pending) return;
    this.#pending.delete(frame.id);
    if (frame.ok) pending.resolve(frame.value);
    else pending.reject(new Error(frame.error));
  }

  call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    if (!readEndpoint()) {
      return Promise.reject(new Error("no Dray host configured"));
    }
    this.#open();
    const id = this.#nextId++;
    const frame = JSON.stringify({ id, type: "call", cmd, args: args ?? {} });
    return new Promise<T>((resolve, reject) => {
      this.#pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
      // A frame sent into a socket still opening is lost without an error, so
      // queue until onopen rather than trusting readyState alone.
      if (this.#socket?.readyState === WebSocket.OPEN) this.#socket.send(frame);
      else this.#queue.push(frame);
    });
  }

  subscribe<T>(event: string, handler: (event: { payload: T }) => void): Promise<UnlistenFn> {
    this.#open();
    let handlers = this.#handlers.get(event);
    if (!handlers) {
      handlers = new Set();
      this.#handlers.set(event, handlers);
    }
    const erased = handler as (event: { payload: unknown }) => void;
    handlers.add(erased);
    return Promise.resolve(() => {
      handlers.delete(erased);
    });
  }

  assetUrl(path: string): string {
    const endpoint = readEndpoint();
    // An image is fetched by the webview's own `<img>`, which can carry no
    // header, so the credential has to ride the query string — which is why it
    // is a per-connection ticket and not the reader's token. Before one has
    // arrived there is nothing to sign with, and an empty `src` draws nothing
    // rather than sending an unauthorised request.
    if (!endpoint || !this.#assetTicket) return "";
    const base = endpoint.url.replace(/^ws/, "http").replace(/\/$/, "");
    const ticket = encodeURIComponent(this.#assetTicket);
    return `${base}/asset?ticket=${ticket}&path=${encodeURIComponent(path)}`;
  }
}

export const remote = new RemoteTransport();
