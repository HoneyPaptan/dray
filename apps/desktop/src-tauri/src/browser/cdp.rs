//! One DevTools connection, and the request/reply correlation over it.
//!
//! The wire is the same protocol `cef/automation.rs` already speaks — a JSON
//! object carrying `id`, `method` and `params` in, a reply carrying that `id`
//! back — so the verbs written against CEF's channel need no change to run
//! here. What differs is only how the bytes travel: a WebSocket to a browser
//! this app started, rather than a message handed to an embedded one.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
};

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{broadcast, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// How long one call waits before it is reported as timed out.
///
/// The same reading `automation.rs` takes: a page that never answers is a page
/// the reader has to be told about, and a verb that hangs forever is worse than
/// one that fails.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How many events are held for a subscriber that is not reading fast enough.
///
/// A screencast fills this quickly, and dropping the oldest frame is the right
/// loss: a stale frame is worth nothing, where a dropped *reply* would strand a
/// caller — which is why replies go through their own channel and never here.
const EVENT_BUFFER: usize = 256;

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>>;

/// A live DevTools connection to one target.
pub struct Connection {
    outgoing: tokio::sync::mpsc::UnboundedSender<Message>,
    pending: Pending,
    events: broadcast::Sender<Value>,
    next_id: AtomicI64,
}

impl Connection {
    /// Opens a connection to `ws_url` and starts reading it.
    ///
    /// Two tasks rather than one: the reader owns the stream's read half for
    /// its whole life, so a call made from any task can never contend for it.
    /// Writes go through a channel for the same reason.
    pub async fn open(ws_url: &str) -> Result<Self> {
        let (stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .with_context(|| format!("could not open a DevTools connection to {ws_url}"))?;
        let (mut sink, mut source) = stream.split();

        let (outgoing, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(EVENT_BUFFER);

        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = Arc::clone(&pending);
        let reader_events = events.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = source.next().await {
                let Message::Text(text) = message else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                deliver(&reader_pending, &reader_events, value);
            }
            // The browser went away. Every caller still waiting has to be told,
            // or a closed tab reads as a call that simply never returns.
            let waiting: Vec<_> = reader_pending.lock().unwrap().drain().collect();
            for (_, tx) in waiting {
                let _ = tx.send(Err("the browser closed before answering".into()));
            }
        });

        Ok(Self { outgoing, pending, events, next_id: AtomicI64::new(1) })
    }

    /// One CDP call, answered or failed.
    ///
    /// The error is a sentence rather than a type, matching what the verbs in
    /// `automation.rs` already hand back to the CLI.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let message = json!({ "id": id, "method": method, "params": params }).to_string();
        if self.outgoing.send(Message::Text(message.into())).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err("the browser is gone".into());
        }

        match tokio::time::timeout(TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err("the browser closed before answering".into()),
            // Dropped here and not by the reader: an id nobody is waiting for
            // would otherwise sit in the map for the life of the connection.
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("{method} timed out after {}s", TIMEOUT.as_secs()))
            }
        }
    }

    /// Every event the target publishes — console lines, screencast frames,
    /// lifecycle. A subscriber that falls behind loses the oldest, never a reply.
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }
}

/// Routes one inbound frame: a reply to whoever asked for it, anything else to
/// the event stream.
///
/// A reply is told from an event by carrying `id`, which is the protocol's own
/// rule rather than a guess about method names.
fn deliver(pending: &Pending, events: &broadcast::Sender<Value>, value: Value) {
    let Some(id) = value.get("id").and_then(Value::as_i64) else {
        // No subscriber is the ordinary state — nothing is watching until a
        // screencast starts — so a failed send here is not an error.
        let _ = events.send(value);
        return;
    };

    let Some(tx) = pending.lock().unwrap().remove(&id) else {
        return;
    };

    // CDP reports a refusal in the envelope rather than by failing the
    // transport, so the message it carries is the only thing that names the
    // cure: a bad selector, a tab that has navigated away.
    let answer = match value.get("error") {
        Some(error) => Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the browser refused that call")
            .to_string()),
        None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
    };
    let _ = tx.send(answer);
}

/// The page targets a browser endpoint is currently showing.
///
/// Read over HTTP rather than asked for with `Target.getTargets`, because this
/// is what answers *before* any connection exists — it is how the first tab's
/// own WebSocket URL is found.
pub async fn targets(endpoint: &str) -> Result<Vec<Value>> {
    let body: Value = reqwest::get(format!("{endpoint}/json/list"))
        .await
        .context("could not ask the browser for its tabs")?
        .json()
        .await
        .context("the browser's tab list was not JSON")?;
    body.as_array().cloned().ok_or_else(|| anyhow!("the browser's tab list was not a list"))
}
