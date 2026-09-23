//! The phone's way in.
//!
//! A websocket server inside the desktop app. A remote client speaks the same
//! vocabulary the webview does — one frame per `invoke`, one frame per event —
//! so the React frontend runs unchanged on a phone with its transport pointed
//! here instead of at its own process.
//!
//! **Commands are executed by the desktop webview, not by this module.** Tauri
//! offers no way to dispatch a registered command by name at runtime, so a
//! second dispatcher here would be 92 hand-written arms that fall out of step
//! the first time a command is added. Relaying instead costs one hop and stays
//! correct by construction. The price is stated rather than hidden: the desktop
//! window has to be open for the phone to reach anything.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

/// Events the frontend subscribes to. A remote client is sent every one of
/// them; the set is stated here because Tauri's listener is per name and there
/// is no "every event" hook to lean on.
const FORWARDED_EVENTS: &[&str] = &[
    "agent_event",
    "session_status",
    "session_created",
    "session_title",
    "models_changed",
    "notification_activated",
    "doc_changed",
    "update_status",
    "check_update_requested",
    "quit_requested",
    "transcription_download_progress",
    "browser_tabs",
    "browser_frame",
];

const DEFAULT_PORT: u16 = 8787;

/// What a client sends.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientFrame {
    Call {
        id: u64,
        cmd: String,
        #[serde(default)]
        args: Value,
    },
}

/// What the desktop webview is asked to run on a client's behalf.
#[derive(Clone, Serialize)]
pub struct RemoteCall {
    pub token: u64,
    pub cmd: String,
    pub args: Value,
}

struct Client {
    outbound: mpsc::UnboundedSender<String>,
}

/// One caller waiting on the webview, and where its answer has to go back to.
struct Waiting {
    client: u64,
    id: u64,
}

#[derive(Default)]
pub struct RemoteServer {
    clients: Mutex<HashMap<u64, Client>>,
    waiting: Mutex<HashMap<u64, Waiting>>,
    /// Asset ticket to the client it was minted for.
    ///
    /// Images are fetched by the webview's own `<img>`, which can carry no
    /// header, so that one credential has to ride the query string. It is not
    /// the reader's token: that opens the websocket, which can create sessions
    /// and send prompts, and a URL ends up in the DOM and in any log between
    /// here and there. A ticket reads assets, nothing else, and dies with the
    /// connection that was given it.
    tickets: Mutex<HashMap<String, u64>>,
    next_client: AtomicU64,
    next_token: AtomicU64,
}

impl RemoteServer {
    fn send_to(&self, client: u64, frame: String) {
        let clients = self.clients.lock().unwrap();
        if let Some(entry) = clients.get(&client) {
            let _ = entry.outbound.send(frame);
        }
    }

    fn broadcast(&self, frame: &str) {
        let clients = self.clients.lock().unwrap();
        for entry in clients.values() {
            let _ = entry.outbound.send(frame.to_string());
        }
    }

    /// Hand a client's answer back. Called by the `remote_reply` command once
    /// the desktop webview has run the command.
    pub fn reply(&self, token: u64, ok: bool, value: Value) {
        let waiting = self.waiting.lock().unwrap().remove(&token);
        let Some(waiting) = waiting else { return };
        let frame = if ok {
            json!({ "id": waiting.id, "ok": true, "value": value })
        } else {
            let message = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
            json!({ "id": waiting.id, "ok": false, "error": message })
        };
        self.send_to(waiting.client, frame.to_string());
    }

    /// Drop every call a departing client was waiting on, so the map cannot
    /// grow for the life of the process on a phone that reconnects all day.
    fn forget_client(&self, client: u64) {
        self.clients.lock().unwrap().remove(&client);
        self.waiting.lock().unwrap().retain(|_, w| w.client != client);
        self.tickets.lock().unwrap().retain(|_, owner| *owner != client);
    }

    fn mint_ticket(&self, client: u64) -> String {
        let ticket = uuid::Uuid::new_v4().simple().to_string();
        self.tickets.lock().unwrap().insert(ticket.clone(), client);
        ticket
    }

    fn knows_ticket(&self, ticket: &str) -> bool {
        self.tickets.lock().unwrap().contains_key(ticket)
    }
}

/// The shared secret a client has to present. Minted once and kept at `0600`.
///
/// Mode rides the create rather than a later `set_permissions`, or the token is
/// world-readable for the window between the two.
pub fn token(dir: &Path) -> Result<String> {
    let path = dir.join("remote-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let minted = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    write_private(&path, minted.as_bytes())?;
    Ok(minted)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let temp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .or_else(|_| {
            let _ = std::fs::remove_file(&temp);
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)
        })
        .with_context(|| format!("create {}", temp.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Every address this host should answer on: loopback, plus the tailnet address
/// if one exists. Never `0.0.0.0` — that would put the agent runtime on whatever
/// network the laptop is sitting on.
fn bind_addresses(port: u16) -> Vec<SocketAddr> {
    let mut addrs = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)];
    if let Some(ip) = tailnet_ip() {
        addrs.push(SocketAddr::new(IpAddr::V4(ip), port));
    }
    addrs
}

fn tailnet_ip() -> Option<Ipv4Addr> {
    let output = std::process::Command::new("tailscale").args(["ip", "-4"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).lines().next()?.trim().parse().ok()
}

/// Start listening. Any failure costs the feature and never the app.
pub fn serve(app: AppHandle) {
    if std::env::var_os("DRAY_NO_SERVE").is_some() {
        return;
    }
    let port = std::env::var("DRAY_SERVE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let dir = match std::env::home_dir() {
        Some(home) => home.join(".dray"),
        None => return,
    };
    let token = match token(&dir) {
        Ok(token) => token,
        Err(e) => {
            eprintln!("[serve] no remote token, phone access is off: {e:#}");
            return;
        }
    };

    forward_events(&app);

    for addr in bind_addresses(port) {
        let app = app.clone();
        let token = token.clone();
        tauri::async_runtime::spawn(async move {
            let listener = match wait_for_port(addr).await {
                Some(listener) => listener,
                None => return,
            };
            eprintln!("[serve] listening on ws://{addr}");
            accept_loop(listener, app, token).await;
        });
    }
}

/// How often a taken port is tried again.
const REBIND_EVERY: Duration = Duration::from_secs(5);

/// The listener for `addr`, waiting for the port if something else holds it.
///
/// A bind that failed once used to cost phone access for the life of the
/// process, silently: another program of the reader's own held 8787, Dray said
/// so in one line of stderr nobody reads, and the phone sat on a connect screen
/// with no way to learn why. The port is fixed rather than moved because the
/// mobile build bakes it in, so healing means waiting for it rather than
/// choosing another.
///
/// Only "already in use" is waited on. Every other failure — a permission
/// refusal, an address this machine no longer has — is a standing answer, and
/// retrying it forever would be a loop that never ends and never says anything
/// new.
async fn wait_for_port(addr: SocketAddr) -> Option<TcpListener> {
    let mut said = false;
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => return Some(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Once, not every five seconds: this is an ordinary state on a
                // machine running something else on the port, and a line per
                // attempt would bury everything else in the log.
                if !said {
                    said = true;
                    eprintln!("[serve] {addr} is taken, waiting for it: {e}");
                }
                tokio::time::sleep(REBIND_EVERY).await;
            }
            Err(e) => {
                eprintln!("[serve] cannot bind {addr}: {e}");
                return None;
            }
        }
    }
}

/// Mirror every forwarded event onto every connected client.
fn forward_events(app: &AppHandle) {
    for name in FORWARDED_EVENTS {
        let app = app.clone();
        let name = *name;
        app.clone().listen_any(name, move |event| {
            let payload: Value = serde_json::from_str(event.payload()).unwrap_or(Value::Null);
            let frame = json!({ "event": name, "payload": payload }).to_string();
            if let Some(server) = app.try_state::<Arc<RemoteServer>>() {
                server.broadcast(&frame);
            }
        });
    }
}

async fn accept_loop(listener: TcpListener, app: AppHandle, token: String) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let app = app.clone();
        let token = token.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = handle(stream, app, token).await {
                eprintln!("[serve] connection ended: {e:#}");
            }
        });
    }
}

/// One connection, either a websocket or a single asset read.
///
/// Peeked rather than read, so the bytes are still in the socket for the
/// websocket handshake to parse for itself.
async fn handle(stream: TcpStream, app: AppHandle, token: String) -> Result<()> {
    let mut peeked = [0u8; 1024];
    let read = stream.peek(&mut peeked).await?;
    let head = String::from_utf8_lossy(&peeked[..read]).to_string();

    let server = app
        .try_state::<Arc<RemoteServer>>()
        .map(|s| s.inner().clone())
        .context("remote server state missing")?;

    if head.to_ascii_lowercase().contains("upgrade: websocket") {
        serve_socket(stream, app, token, server).await
    } else {
        serve_asset(stream, &head, read, &server).await
    }
}

async fn serve_socket(
    stream: TcpStream,
    app: AppHandle,
    token: String,
    server: Arc<RemoteServer>,
) -> Result<()> {
    let socket = tokio_tungstenite::accept_async(stream).await?;
    let (mut sink, mut source) = socket.split();

    // The token rides the query string: a browser cannot put a header on a
    // websocket, so there is nowhere else for it to go.
    let authorised = Arc::new(Mutex::new(false));

    let id = server.next_client.fetch_add(1, Ordering::Relaxed);
    // Registered only once the token has been seen. `broadcast` writes to every
    // client in this map, so a connection joining it before authenticating is
    // handed the whole event stream — every agent_event, every transcript — for
    // free. The channel exists from the start; membership does not.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let writer = tauri::async_runtime::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if sink.send(tokio_tungstenite::tungstenite::Message::text(frame)).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = source.next().await {
        let Ok(text) = message.into_text() else { continue };
        if !*authorised.lock().unwrap() {
            // The handshake request is gone by now, so the first frame carries
            // the token instead.
            let ok = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_owned))
                .is_some_and(|presented| constant_time_eq(&presented, &token));
            if !ok {
                break;
            }
            *authorised.lock().unwrap() = true;
            server.clients.lock().unwrap().insert(id, Client { outbound: tx.clone() });
            let ticket = server.mint_ticket(id);
            server.send_to(
                id,
                json!({ "event": "remote_ready", "payload": { "asset": ticket } }).to_string(),
            );
            continue;
        }
        let Ok(frame) = serde_json::from_str::<ClientFrame>(&text) else { continue };
        let ClientFrame::Call { id: call_id, cmd, args } = frame;
        let call_token = server.next_token.fetch_add(1, Ordering::Relaxed);
        server.waiting.lock().unwrap().insert(call_token, Waiting { client: id, id: call_id });
        let _ = app.emit("remote_call", RemoteCall { token: call_token, cmd, args });
    }

    server.forget_client(id);
    writer.abort();
    Ok(())
}

/// Compare two secrets without letting the clock say where they differ.
///
/// Folded rather than short-circuited: `==` on a `String` returns at the first
/// byte that differs, so the time it takes is a measurement of how much of the
/// token a caller has guessed. Length is folded in the same way rather than
/// checked first, which would leak it on its own.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

/// One file, for the images a transcript draws. Read straight rather than
/// streamed: the cap is what keeps that honest.
const MAX_ASSET: u64 = 16 * 1024 * 1024;

async fn serve_asset(
    mut stream: TcpStream,
    head: &str,
    peeked: usize,
    server: &RemoteServer,
) -> Result<()> {
    // Byte count, never `head.len()`: the head was decoded lossily, so a
    // non-ASCII byte makes the two disagree and the drain then eats into the
    // next request or stops short of this one.
    let mut drain = vec![0u8; peeked];
    stream.read_exact(&mut drain).await?;

    let target = head.split_whitespace().nth(1).unwrap_or("");
    let query: HashMap<_, _> = target
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), percent_decode(v)))
        .collect();

    // A ticket, never the reader's own token: see `RemoteServer::tickets`.
    if !query.get("ticket").is_some_and(|t| server.knows_ticket(t)) {
        return respond(&mut stream, 403, "text/plain", b"forbidden").await;
    }
    let Some(path) = query.get("path") else {
        return respond(&mut stream, 400, "text/plain", b"no path").await;
    };

    let Some(path) = attachment_path(path) else {
        return respond(&mut stream, 403, "text/plain", b"forbidden").await;
    };
    let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX_ASSET).unwrap_or(true);
    if too_big {
        return respond(&mut stream, 404, "text/plain", b"not found").await;
    }
    match std::fs::read(&path) {
        Ok(bytes) => respond(&mut stream, 200, content_type(&path), &bytes).await,
        Err(_) => respond(&mut stream, 404, "text/plain", b"not found").await,
    }
}

/// The one directory this endpoint may read, or `None`.
///
/// The path arrives from the client, and this reads a file and sends it back,
/// so without the check a ticket reads anything the user can — `~/.ssh` and the
/// rest. Transcript images are the whole purpose, and they are archived under
/// `~/.dray/attachments`, so that is the boundary. The same reading
/// `recordings::read` takes of its own directory.
///
/// Canonicalized on both sides: `..` is what a traversal is spelled with, and a
/// symlinked home makes a written-path comparison find nothing.
fn attachment_path(raw: &str) -> Option<PathBuf> {
    let root = std::env::home_dir()?.join(".dray").join("attachments").canonicalize().ok()?;
    let path = PathBuf::from(raw).canonicalize().ok()?;
    path.starts_with(&root).then_some(path)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

async fn respond(stream: &mut TcpStream, status: u16, mime: &str, body: &[u8]) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_percent_escaped_path() {
        assert_eq!(percent_decode("%2Fhome%2Fa%20b.png"), "/home/a b.png");
    }

    #[test]
    fn secrets_compare_without_short_circuiting() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        // Length alone must not decide it, or the answer leaks on its own.
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn an_asset_path_outside_the_attachments_directory_is_refused() {
        assert!(attachment_path("/etc/passwd").is_none());
        assert!(attachment_path("/home/../etc/passwd").is_none());
        assert!(attachment_path("").is_none());
    }

    #[test]
    fn loopback_is_always_bound() {
        let addrs = bind_addresses(1234);
        assert!(addrs.iter().any(|a| a.ip().is_loopback()));
        assert!(!addrs.iter().any(|a| a.ip().is_unspecified()));
    }
}
