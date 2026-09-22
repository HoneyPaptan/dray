//! The Chromium-over-CDP half of the browser verbs.
//!
//! [`super::automation`] holds every verb and speaks CDP alone; this is the
//! backend that carries those messages on a platform with no embedded
//! Chromium. It answers the same names [`crate::cef::devtools`] does, so the
//! verbs never learn which browser they reached.
//!
//! **What a page reports, this has to remember.** CEF keeps url, title and
//! loading state for Dray, off callbacks it makes on its own; a browser Dray
//! merely started reports the same facts as DevTools events and nothing keeps
//! them. So every tab gets a watcher task, and what it writes is what
//! `tab_state` reads. It is a task rather than a call per question because
//! `tab_state` is synchronous — `wait_loaded` polls it — and a page's title
//! cannot be asked for without awaiting.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::Tab;

/// What the verbs ask about a tab, kept current by its watcher.
#[derive(Default)]
struct Record {
    session: String,
    /// CDP's own id for the page, which is what the browser channel names it
    /// by. The `i32` beside it is this app's, and nothing outside knows it.
    target: String,
    url: String,
    title: String,
    loading: bool,
    /// What the page has logged since `console`/`errors` last drained it.
    console: Vec<(bool, String)>,
}

/// Every open tab, by the id `automation.rs` addresses it with. Keyed
/// globally rather than per session because `tab_state` is asked by id alone.
static TABS: Mutex<Option<HashMap<i32, Record>>> = Mutex::new(None);
/// Which tab each session's verbs land on.
static ACTIVE: Mutex<Option<HashMap<String, i32>>> = Mutex::new(None);
/// Matches CEF's own cap: a page in a loop must not grow this without bound.
const MAX_CONSOLE: usize = 200;

fn with_tabs<T>(f: impl FnOnce(&mut HashMap<i32, Record>) -> T) -> T {
    f(TABS.lock().unwrap().get_or_insert_with(HashMap::new))
}

/// One CDP call on one of a session's tabs.
pub async fn cdp(session: &str, tab: i32, method: &str, params: Value) -> Result<Value, String> {
    super::cdp(session, tab, method, params).await
}

pub fn tab_state(tab: i32) -> Option<(String, String, bool)> {
    with_tabs(|tabs| tabs.get(&tab).map(|r| (r.url.clone(), r.title.clone(), r.loading)))
}

pub fn tabs(session: &str) -> Vec<Tab> {
    let active = active(session);
    let mut open: Vec<Tab> = with_tabs(|tabs| {
        tabs.iter()
            .filter(|(_, r)| r.session == session)
            .map(|(id, r)| Tab {
                id: *id,
                active: active == Some(*id),
                url: r.url.clone(),
                title: r.title.clone(),
            })
            .collect()
    });
    // Ids are minted in order, so this is the order they were opened in —
    // what the reader saw, rather than a hash map's.
    open.sort_by_key(|t| t.id);
    open
}

pub fn active(session: &str) -> Option<i32> {
    ACTIVE.lock().unwrap().as_ref()?.get(session).copied()
}

fn set_active(session: &str, tab: Option<i32>) {
    let mut guard = ACTIVE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match tab {
        Some(tab) => {
            map.insert(session.to_string(), tab);
        }
        None => {
            map.remove(session);
        }
    }
}

/// Opens `url`, in the session's active tab or in a new one.
///
/// Loading is marked here rather than left to the page's own event, and that
/// is what `wait_loaded` rests on: a navigation takes a moment to commit, so
/// a verb asking "still loading?" straight after this would otherwise read
/// the *old* page as settled and carry on against it.
pub async fn open(session: &str, url: String, new_tab: bool) -> Result<(), String> {
    let instance = super::instance(session).await.map_err(|err| format!("{err:#}"))?;
    let existing = active(session).filter(|_| !new_tab);
    match existing {
        Some(tab) => {
            with_tabs(|tabs| {
                if let Some(record) = tabs.get_mut(&tab) {
                    record.loading = true;
                }
            });
            cdp(session, tab, "Page.navigate", json!({ "url": url })).await?;
        }
        None => {
            let (tab, page) =
                instance.open_tab(&url).await.map_err(|err| format!("{err:#}"))?;
            // A blank tab is not navigated — it is already what was asked
            // for — so nothing will report it loaded, and marking it loading
            // would leave every later verb waiting out the load timeout.
            let loading = url != "about:blank";
            with_tabs(|tabs| {
                tabs.insert(
                    tab,
                    Record {
                    session: session.into(),
                    target: page.target.clone(),
                    loading,
                    ..Record::default()
                },
                );
            });
            watch(tab, Arc::clone(&page.connection));
            set_active(session, Some(tab));
        }
    }
    Ok(())
}

pub async fn nav(session: &str, verb: &str) -> Result<(), String> {
    let tab = active(session).ok_or("no tab is open in this session's browser")?;
    with_tabs(|tabs| {
        if let Some(record) = tabs.get_mut(&tab) {
            record.loading = true;
        }
    });
    if verb == "reload" {
        cdp(session, tab, "Page.reload", json!({})).await?;
        return Ok(());
    }

    // CDP has no back or forward verb: history is a list and a position in
    // it, and moving is naming the entry to go to. An entry off either end is
    // the ordinary state at the start or end of a history, so it answers
    // rather than failing.
    let history = cdp(session, tab, "Page.getNavigationHistory", json!({})).await?;
    let index = history["currentIndex"].as_i64().unwrap_or(0);
    let entries = history["entries"].as_array().cloned().unwrap_or_default();
    let wanted = if verb == "back" { index - 1 } else { index + 1 };
    let Some(entry) = usize::try_from(wanted).ok().and_then(|i| entries.get(i)) else {
        with_tabs(|tabs| {
            if let Some(record) = tabs.get_mut(&tab) {
                record.loading = false;
            }
        });
        return Ok(());
    };
    let id = entry["id"].clone();
    cdp(session, tab, "Page.navigateToHistoryEntry", json!({ "entryId": id })).await?;
    Ok(())
}

pub async fn close_tab(session: &str, tab: i32) -> Result<(), String> {
    let instance = super::instance(session).await.map_err(|err| format!("{err:#}"))?;
    instance.close_tab(tab).await.map_err(|err| format!("{err:#}"))?;
    with_tabs(|tabs| tabs.remove(&tab));
    if active(session) == Some(tab) {
        // The newest surviving tab, or none — the reader's next verb has to
        // land somewhere, and leaving the session pointing at a closed tab
        // would report every one of them as "that tab is gone".
        set_active(session, tabs(session).last().map(|t| t.id));
    }
    Ok(())
}

pub async fn activate(session: &str, tab: i32) -> Result<(), String> {
    let instance = super::instance(session).await.map_err(|err| format!("{err:#}"))?;
    instance.activate_tab(tab).await.map_err(|err| format!("{err:#}"))?;
    set_active(session, Some(tab));
    Ok(())
}

pub fn console_drain(tab: i32) -> Vec<(bool, String)> {
    with_tabs(|tabs| tabs.get_mut(&tab).map(|r| std::mem::take(&mut r.console)).unwrap_or_default())
}

/// Nothing to reveal: the browser is headless, so no widget can be hiding
/// the input this is about to dispatch.
pub async fn before_input(_tab: i32) -> Result<(), String> {
    Ok(())
}

pub fn after_input() {}

pub fn shots_dir() -> std::path::PathBuf {
    std::env::home_dir().unwrap_or_default().join(".dray").join("browser").join("shots")
}

/// Nothing holds a capture slot here yet. The embedded pane needs one because
/// the widget the reader is watching is the one being resized for the shot;
/// a screencast has no such widget, and giving it one is Phase 3's question.
///
/// Not a `()`, which is `Copy` and makes the caller's `drop` a no-op the
/// compiler warns about rather than a slot being released.
pub struct Slot;

pub async fn capture_guard() -> Slot {
    Slot
}

pub fn shutter_is_open() -> bool {
    false
}

pub async fn try_capture_guard() -> Result<Slot, String> {
    Ok(Slot)
}

pub async fn cover(_session: &str) -> u64 {
    0
}

pub async fn uncover(_session: &str, _shot: u64) {}

/// Keeps every tab's title and url current off the browser's own channel.
///
/// A page-level connection reports navigation and nothing else, so a script
/// setting `document.title` — which is most single-page apps, on every route
/// change — left the tab strip and `get title` naming the page as it was when
/// it loaded. `Target.targetInfoChanged` is the one event that carries it.
pub fn watch_targets(connection: Arc<super::cdp::Connection>) {
    let mut events = connection.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            if event.get("method").and_then(Value::as_str) != Some("Target.targetInfoChanged") {
                continue;
            }
            let info = &event["params"]["targetInfo"];
            let Some(target) = info["targetId"].as_str() else { continue };
            with_tabs(|tabs| {
                let Some(record) = tabs.values_mut().find(|r| r.target == target) else { return };
                if let Some(title) = info["title"].as_str() {
                    record.title = title.to_string();
                }
                if let Some(url) = info["url"].as_str() {
                    record.url = url.to_string();
                }
            });
        }
    });
}

/// Keeps one tab's record current off the page's own events.
///
/// The task ends when the connection does, which is what makes it safe to
/// spawn one per tab and never join it.
fn watch(tab: i32, connection: Arc<super::cdp::Connection>) {
    let mut events = connection.subscribe();
    tokio::spawn(async move {
        // What the tab already is, before it says anything. A blank tab
        // reports no load event at all, so without this its url would stay
        // empty and `tabs` would draw a row naming nothing.
        settle(tab, &connection, None).await;
        while let Ok(event) = events.recv().await {
            let method = event.get("method").and_then(Value::as_str).unwrap_or_default();
            let params = event.get("params").cloned().unwrap_or(Value::Null);
            match method {
                // Only the top-level frame: a subframe navigating is the page
                // loading something, not the reader arriving somewhere.
                "Page.frameNavigated" if params.pointer("/frame/parentId").is_none() => {
                    let url = params
                        .pointer("/frame/url")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    with_tabs(|tabs| {
                        if let Some(record) = tabs.get_mut(&tab) {
                            record.url = url;
                            // The old page's title over a new page's URL is
                            // worse than no title: both are drawn as one line.
                            record.title.clear();
                            record.loading = true;
                        }
                    });
                }
                "Page.loadEventFired" => settle(tab, &connection, Some(false)).await,
                "Runtime.consoleAPICalled" => {
                    let error = matches!(
                        params["type"].as_str(),
                        Some("error") | Some("warning") | Some("assert")
                    );
                    record_line(tab, error, console_text(&params["args"]));
                }
                "Runtime.exceptionThrown" => {
                    let text = params
                        .pointer("/exceptionDetails/exception/description")
                        .or_else(|| params.pointer("/exceptionDetails/text"))
                        .and_then(Value::as_str)
                        .unwrap_or("an exception was thrown")
                        .to_string();
                    record_line(tab, true, text);
                }
                _ => {}
            }
        }
    });
}

/// Reads the page's own url and title into the record, and sets the loading
/// flag where the caller knows it. The title is the reason this is a call
/// rather than a field off an event: no CDP event carries one, and
/// `Page.getNavigationHistory` answers the title the *history* recorded
/// rather than the one the document is wearing now.
async fn settle(tab: i32, connection: &super::cdp::Connection, loading: Option<bool>) {
    let read = connection
        .call(
            "Runtime.evaluate",
            json!({
                "expression": "({ url: location.href, title: document.title })",
                "returnByValue": true,
            }),
        )
        .await
        .ok();
    with_tabs(|tabs| {
        let Some(record) = tabs.get_mut(&tab) else { return };
        if let Some(loading) = loading {
            record.loading = loading;
        }
        let Some(value) = read.as_ref().and_then(|v| v.pointer("/result/value")) else { return };
        if let Some(url) = value["url"].as_str() {
            record.url = url.to_string();
        }
        if let Some(title) = value["title"].as_str() {
            record.title = title.to_string();
        }
    });
}

/// One console line's worth of text. A logged object arrives described rather
/// than serialized, so `description` is what an agent can read; `value` is
/// what a string or a number carries.
fn console_text(args: &Value) -> String {
    args.as_array()
        .map(|args| {
            args.iter()
                .map(|arg| match (&arg["value"], arg["description"].as_str()) {
                    (Value::String(text), _) => text.clone(),
                    (Value::Null, Some(described)) => described.to_string(),
                    (value, _) => value.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

fn record_line(tab: i32, error: bool, text: String) {
    with_tabs(|tabs| {
        if let Some(record) = tabs.get_mut(&tab) {
            if record.console.len() >= MAX_CONSOLE {
                record.console.remove(0);
            }
            record.console.push((error, text));
        }
    });
}

/// Forgets a session's tabs. Called with the browser itself, so nothing here
/// outlives the process it described.
pub fn forget_session(session: &str) {
    with_tabs(|tabs| tabs.retain(|_, record| record.session != session));
    set_active(session, None);
}
