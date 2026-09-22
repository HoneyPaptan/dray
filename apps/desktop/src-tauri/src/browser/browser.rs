//! The in-app browser on every platform without an embedded Chromium.
//!
//! One headless browser per session, driven over the DevTools protocol and
//! drawn inside Dray from a screencast. The verbs live in
//! [`crate::cef::automation`] and are untouched by this: they were written
//! against a DevTools channel, and this is another one.
//!
//! **Why not embed.** CEF has no embedded Wayland support, so the only embedded
//! route on Linux is forcing the whole app under XWayland. And a native view is
//! pixels in one process's window — it can never be sent to a phone, where a
//! screencast frame can. One pipe serves the pane and the phone rather than two
//! implementations of the same feature.

#[path = "cdp.rs"]
pub mod cdp;
#[path = "launch.rs"]
pub mod launch;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use cdp::Connection;

/// The browsers this app has started, one per session.
///
/// A `tokio::sync::Mutex` rather than a `std` one, and that is load-bearing:
/// starting a browser is awaited under this lock, so two sends arriving
/// together on one session cannot each spawn a Chromium and leave one of them
/// orphaned with a profile nobody will clean up.
static SESSIONS: LazyLock<Mutex<HashMap<String, Arc<Instance>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// One session's browser, and the page connections open against it.
pub struct Instance {
    /// Kept so the process dies with it — `kill_on_drop` is what makes closing
    /// a session take its browser, rather than leaving a headless Chromium
    /// holding a profile for the life of the login.
    _child: tokio::process::Child,
    endpoint: String,
    tabs: Mutex<HashMap<i32, Arc<Connection>>>,
}

/// Where a session's browser keeps its profile.
///
/// The directory CEF already used, since the two can never run on one machine
/// at once — and a reader who switches builds should not find their logins in
/// a directory the other half cannot see.
fn profile_of(session: &str) -> Result<PathBuf> {
    let home = std::env::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    Ok(home.join(".dray").join("browser").join(session))
}

/// The session's browser, started if it is not already running.
pub async fn instance(session: &str) -> Result<Arc<Instance>> {
    let mut sessions = SESSIONS.lock().await;
    if let Some(running) = sessions.get(session) {
        return Ok(Arc::clone(running));
    }

    let binary = launch::resolve().ok_or_else(|| {
        anyhow!("no Chromium found. Install chromium, google-chrome or brave to use the browser")
    })?;
    let started = launch::start(&binary, &profile_of(session)?).await?;
    let instance = Arc::new(Instance {
        _child: started.child,
        endpoint: started.endpoint,
        tabs: Mutex::new(HashMap::new()),
    });
    sessions.insert(session.to_string(), Arc::clone(&instance));
    Ok(instance)
}

impl Instance {
    /// Opens a page at `url` and answers the id this app addresses it by.
    ///
    /// The id is ours rather than CDP's target id: `automation.rs` addresses a
    /// tab by `i32` and every verb in it is written that way, so minting one
    /// here keeps that whole file transport-free.
    pub async fn open_tab(&self, url: &str) -> Result<i32> {
        let created: Value = reqwest::Client::new()
            .put(format!("{}/json/new?{url}", self.endpoint))
            .send()
            .await
            .context("could not ask the browser for a tab")?
            .json()
            .await
            .context("the browser's answer was not JSON")?;

        let ws = created
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("the browser opened a tab with no DevTools URL"))?;

        let connection = Arc::new(Connection::open(ws).await?);
        // Asked for on every tab rather than on demand: both are how the page
        // reports things nobody requested, so a subscriber attaching later
        // would miss whatever happened before it.
        let _ = connection.call("Page.enable", json!({})).await;
        let _ = connection.call("Runtime.enable", json!({})).await;

        let mut tabs = self.tabs.lock().await;
        let id = tabs.keys().max().copied().unwrap_or(0) + 1;
        tabs.insert(id, connection);
        Ok(id)
    }

    /// The connection for `tab`, or `None` where it has been closed.
    pub async fn tab(&self, tab: i32) -> Option<Arc<Connection>> {
        self.tabs.lock().await.get(&tab).cloned()
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// One CDP call on one of a session's tabs.
///
/// The signature `automation.rs` already calls its embedded counterpart by, so
/// the verbs above it need no knowledge of which half answered.
pub async fn cdp(session: &str, tab: i32, method: &str, params: Value) -> Result<Value, String> {
    let instance = instance(session).await.map_err(|err| format!("{err:#}"))?;
    let Some(connection) = instance.tab(tab).await else {
        return Err("that tab is gone".into());
    };
    connection.call(method, params).await
}

/// Stops a session's browser and forgets it.
///
/// Called when a session is settled or deleted. The profile is left on disk:
/// an unsettled session resuming should find the logins it had.
pub async fn close(session: &str) {
    SESSIONS.lock().await.remove(session);
}

/// Live, and `#[ignore]`d for it: this starts a real browser.
///
/// The one test that proves the transport rather than the types — run it when
/// Chromium changes what `/json/new` or `DevToolsActivePort` answer.
/// `cargo test --lib browser:: -- --ignored --nocapture`
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore = "starts a real browser"]
    async fn drives_a_page() {
        let binary = launch::resolve().expect("no Chromium installed");
        println!("browser: {}", binary.display());

        let profile = std::env::temp_dir().join(format!("dray-browser-test-{}", std::process::id()));
        let started = launch::start(&binary, &profile).await.expect("could not start");
        println!("endpoint: {}", started.endpoint);

        let instance = Instance {
            _child: started.child,
            endpoint: started.endpoint,
            tabs: Mutex::new(HashMap::new()),
        };

        let tab = instance
            .open_tab("data:text/html,<title>hello dray</title><p id=x>ok")
            .await
            .expect("could not open a tab");
        let connection = instance.tab(tab).await.expect("tab vanished");

        let title = connection
            .call(
                "Runtime.evaluate",
                json!({ "expression": "document.title", "returnByValue": true }),
            )
            .await
            .expect("evaluate failed");
        assert_eq!(title.pointer("/result/value").and_then(Value::as_str), Some("hello dray"));

        // The screencast is the whole reason for this route, so the test that
        // proves the transport proves a frame arrives too rather than leaving
        // that to be discovered from the UI.
        let mut events = connection.subscribe();
        connection
            .call("Page.startScreencast", json!({ "format": "jpeg", "quality": 60 }))
            .await
            .expect("startScreencast failed");

        let frame = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let event = events.recv().await.expect("event stream closed");
                if event.get("method").and_then(Value::as_str) == Some("Page.screencastFrame") {
                    return event;
                }
            }
        })
        .await
        .expect("no screencast frame arrived");

        let data = frame.pointer("/params/data").and_then(Value::as_str).expect("frame had no data");
        println!("first frame: {} base64 chars", data.len());
        assert!(data.len() > 1000, "a frame of a real page is not this small");

        let _ = std::fs::remove_dir_all(&profile);
    }
}
