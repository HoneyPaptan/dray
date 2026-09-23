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

#[path = "automation.rs"]
pub mod automation;
#[path = "backend.rs"]
pub mod backend;
#[path = "cdp.rs"]
pub mod cdp;
#[path = "launch.rs"]
pub mod launch;
#[path = "pane.rs"]
pub mod pane;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc, LazyLock, OnceLock,
    },
};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use cdp::Connection;

/// What a verb needs to know about one tab. Both backends answer in this
/// shape — the embedded one reads it off CEF's own callbacks, this one off
/// the page's DevTools events — so [`automation`] never learns which.
pub struct Tab {
    pub id: i32,
    pub active: bool,
    pub url: String,
    pub title: String,
}

/// Where the pane's events go. Set once in `setup`; the backend has no handle
/// of its own, since its callers are the CLI's verbs as often as the webview.
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn install(app: AppHandle) {
    let _ = APP.set(app);
}

/// Emits to the webview, and through `serve` to every phone. Nothing before
/// `install` has anywhere to go, which is only the live tests.
pub(crate) fn emit(name: &str, payload: Value) {
    if let Some(app) = APP.get() {
        let _ = app.emit(name, payload);
    }
}

/// Tab ids are minted across every session, not per browser.
///
/// Load-bearing: `tab_state` is asked by id alone, with no session beside it,
/// so two sessions numbering their own tabs from 1 would have each reading
/// the other's page. CEF's ids are its browser identifiers and unique for the
/// same reason.
static NEXT_TAB: AtomicI32 = AtomicI32::new(1);

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
    tabs: Mutex<HashMap<i32, Arc<Page>>>,
    /// The browser's own DevTools channel, kept open for the life of the
    /// session. It is not for sending on — it is what reports a page's title
    /// changing, which no *page*-level event does.
    _browser: Arc<Connection>,
}

/// One open page: the DevTools channel, and the target id the browser's HTTP
/// endpoint addresses it by. Both are needed — a call goes down the socket,
/// where closing and activating are HTTP verbs on the target.
pub struct Page {
    pub target: String,
    pub connection: Arc<Connection>,
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
    let browser = Arc::new(Connection::open(&browser_socket(&started.endpoint).await?).await?);
    // Without discovery the browser channel reports nothing at all. With it,
    // a page whose script sets `document.title` arrives here as a changed
    // target — the one place that fact is published.
    browser
        .call("Target.setDiscoverTargets", json!({ "discover": true }))
        .await
        .map_err(|message| anyhow!("{message}"))?;
    backend::watch_targets(Arc::clone(&browser));
    let instance = Arc::new(Instance {
        _child: started.child,
        endpoint: started.endpoint,
        tabs: Mutex::new(HashMap::new()),
        _browser: browser,
    });
    sessions.insert(session.to_string(), Arc::clone(&instance));
    Ok(instance)
}

/// The browser-wide DevTools socket, which `/json/version` is the only place
/// that names.
async fn browser_socket(endpoint: &str) -> Result<String> {
    let version: Value = reqwest::get(format!("{endpoint}/json/version"))
        .await
        .context("could not ask the browser for its DevTools socket")?
        .json()
        .await
        .context("the browser's version answer was not JSON")?;
    version
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the browser named no DevTools socket"))
}

impl Instance {
    /// Opens a page at `url` and answers the id this app addresses it by.
    ///
    /// The id is ours rather than CDP's target id: `automation.rs` addresses a
    /// tab by `i32` and every verb in it is written that way, so minting one
    /// here keeps that whole file transport-free.
    /// The tab is created blank and *then* navigated, which is not a round
    /// trip wasted. `PUT /json/new?<url>` starts the load before there is a
    /// socket to watch it on, so a page that finished loading in that gap
    /// would never report `Page.loadEventFired` and the tab would read as
    /// loading for ever — every verb after it waiting out the full timeout.
    /// Navigating after `Page.enable` means the load cannot start before
    /// somebody is listening.
    pub async fn open_tab(&self, url: &str) -> Result<(i32, Arc<Page>)> {
        let created: Value = reqwest::Client::new()
            .put(format!("{}/json/new", self.endpoint))
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
        let target = created
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("the browser opened a tab with no target id"))?
            .to_string();

        let connection = Arc::new(Connection::open(ws).await?);
        // Asked for on every tab rather than on demand: both are how the page
        // reports things nobody requested, so a subscriber attaching later
        // would miss whatever happened before it.
        let _ = connection.call("Page.enable", json!({})).await;
        let _ = connection.call("Runtime.enable", json!({})).await;
        if url != "about:blank" {
            connection
                .call("Page.navigate", json!({ "url": url }))
                .await
                .map_err(|message| anyhow!("{message}"))?;
        }

        let page = Arc::new(Page { target, connection });
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        self.tabs.lock().await.insert(id, Arc::clone(&page));
        Ok((id, page))
    }

    /// The page behind `tab`, or `None` where it has been closed.
    pub async fn page(&self, tab: i32) -> Option<Arc<Page>> {
        self.tabs.lock().await.get(&tab).cloned()
    }

    /// Shuts a tab and forgets it. The browser stays up: a session with no
    /// tabs open is one the reader may open another in.
    pub async fn close_tab(&self, tab: i32) -> Result<()> {
        let Some(page) = self.tabs.lock().await.remove(&tab) else {
            return Ok(());
        };
        reqwest::Client::new()
            .get(format!("{}/json/close/{}", self.endpoint, page.target))
            .send()
            .await
            .context("could not close the tab")?;
        Ok(())
    }

    /// Brings a tab to the front of its browser. Headless, nothing is drawn
    /// by it — what it moves is which page the browser considers focused,
    /// which is what a page's visibility and focus events read.
    pub async fn activate_tab(&self, tab: i32) -> Result<()> {
        let Some(page) = self.tabs.lock().await.get(&tab).cloned() else {
            return Ok(());
        };
        reqwest::Client::new()
            .get(format!("{}/json/activate/{}", self.endpoint, page.target))
            .send()
            .await
            .context("could not activate the tab")?;
        Ok(())
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
    let Some(page) = instance.page(tab).await else {
        return Err("that tab is gone".into());
    };
    page.connection.call(method, params).await
}

/// Stops a session's browser and forgets it.
///
/// Called when a session is settled or deleted. The profile is left on disk:
/// an unsettled session resuming should find the logins it had.
pub async fn close(session: &str) {
    SESSIONS.lock().await.remove(session);
    // The records outlive nothing: dropping the instance kills the browser,
    // so a tab left in the registry would answer `tab_state` about a page no
    // process is holding any more.
    backend::forget_session(session);
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
    async fn a_screencast_frame_arrives() {
        let session = format!("browser-transport-{}", std::process::id());
        let instance = instance(&session).await.expect("could not start a browser");
        println!("endpoint: {}", instance.endpoint());

        let (_, page) = instance
            .open_tab("about:blank")
            .await
            .expect("could not open a tab");

        // The screencast is the whole reason for this route, so the test that
        // proves the transport proves a frame arrives too rather than leaving
        // that to be discovered from the UI.
        let mut events = page.connection.subscribe();
        page.connection
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
        assert!(!data.is_empty(), "a frame carries pixels");

        close(&session).await;
        let _ = std::fs::remove_dir_all(profile_of(&session).unwrap());
    }
}
