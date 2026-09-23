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

use serde::Serialize;
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

/// The pane's screencast: which tab is being drawn for a session, and at what
/// size. One per session, since the pane draws one tab — the active one — and
/// moving the screencast is what `activate` does to it.
#[derive(Clone, Copy)]
struct Cast {
    tab: i32,
    width: u32,
    height: u32,
    scale: f64,
    /// The pane is on a touch screen. The page is laid out as a phone and
    /// touch events are what its input arrives as, so a tap scrolls and a
    /// `viewport` meta tag is honoured — what a reader testing their
    /// frontend from a phone is there to see.
    touch: bool,
}

static CAST: Mutex<Option<HashMap<String, Cast>>> = Mutex::new(None);

/// What the pane draws in its tab strip.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub id: i32,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub active: bool,
}

/// JPEG quality of a screencast frame. A frame lives until the next one, so
/// it is tuned for the wire to a phone rather than for a still.
const FRAME_QUALITY: u32 = 85;

/// Every `Input.*` method the pane may dispatch. The webview is this app's
/// own, so the list guards against a drifted call site rather than an
/// attacker — but a page-level method arriving here would be a verb the
/// CLI already has, reached by a second route.
const INPUT_METHODS: &[&str] = &[
    "Input.dispatchMouseEvent",
    "Input.dispatchTouchEvent",
    "Input.dispatchKeyEvent",
    "Input.insertText",
];

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

pub fn tabs_info(session: &str) -> Vec<TabInfo> {
    let active = active(session);
    let mut open: Vec<TabInfo> = with_tabs(|tabs| {
        tabs.iter()
            .filter(|(_, r)| r.session == session)
            .map(|(id, r)| TabInfo {
                id: *id,
                url: r.url.clone(),
                title: r.title.clone(),
                loading: r.loading,
                active: active == Some(*id),
            })
            .collect()
    });
    open.sort_by_key(|t| t.id);
    open
}

/// Tells the pane the strip has changed. Called from every write to a record,
/// so the pane never polls: a title landing, a load ending and a tab closing
/// all arrive as the whole list, which is the shape the pane draws.
fn publish_tabs(session: &str) {
    super::emit("browser_tabs", json!({ "sessionId": session, "tabs": tabs_info(session) }));
}

fn session_of(tab: i32) -> Option<String> {
    with_tabs(|tabs| tabs.get(&tab).map(|r| r.session.clone()))
}

fn publish_tab(tab: i32) {
    if let Some(session) = session_of(tab) {
        publish_tabs(&session);
    }
}

fn cast_of(session: &str) -> Option<Cast> {
    CAST.lock().unwrap().as_ref()?.get(session).copied()
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
            publish_tabs(session);
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
            publish_tabs(session);
            recast(session).await;
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
    publish_tabs(session);
    match verb {
        "reload" => {
            cdp(session, tab, "Page.reload", json!({})).await?;
            return Ok(());
        }
        "hard_reload" => {
            cdp(session, tab, "Page.reload", json!({ "ignoreCache": true })).await?;
            return Ok(());
        }
        "stop" => {
            cdp(session, tab, "Page.stopLoading", json!({})).await?;
            with_tabs(|tabs| {
                if let Some(record) = tabs.get_mut(&tab) {
                    record.loading = false;
                }
            });
            publish_tabs(session);
            return Ok(());
        }
        _ => {}
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
        publish_tabs(session);
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
    publish_tabs(session);
    recast(session).await;
    Ok(())
}

pub async fn activate(session: &str, tab: i32) -> Result<(), String> {
    let instance = super::instance(session).await.map_err(|err| format!("{err:#}"))?;
    instance.activate_tab(tab).await.map_err(|err| format!("{err:#}"))?;
    set_active(session, Some(tab));
    publish_tabs(session);
    recast(session).await;
    Ok(())
}

// --- The pane's screencast ---------------------------------------------------

/// Starts drawing the session's active tab at `width`×`height` CSS pixels,
/// `scale` device pixels each. Frames arrive as `browser_frame` events off the
/// tab's own watcher; a screencast already running on another tab is stopped,
/// so a session has one.
pub async fn cast(session: &str, width: u32, height: u32, scale: f64, touch: bool) -> Result<(), String> {
    let tab = active(session).ok_or("no tab is open in this session's browser")?;
    let previous = cast_of(session);
    // Capped at the compositor's own scale: the browser renders at
    // `CAST_SCALE` and a frame is only ever scaled *down* to `maxWidth`.
    let cast = Cast {
        tab,
        width: width.max(1),
        height: height.max(1),
        scale: scale.clamp(0.5, super::launch::CAST_SCALE as f64),
        touch,
    };
    CAST.lock().unwrap().get_or_insert_with(HashMap::new).insert(session.to_string(), cast);
    if let Some(old) = previous.filter(|old| old.tab != tab) {
        let _ = cdp(session, old.tab, "Page.stopScreencast", json!({})).await;
    }
    start_cast(session, cast).await
}

/// Stops the session's screencast and forgets its size. The metrics override
/// is left standing: clearing it reflows the page to Chromium's own 800×600,
/// and the next `watch` sets it again anyway.
pub async fn uncast(session: &str) {
    let cast = CAST.lock().unwrap().as_mut().and_then(|m| m.remove(session));
    if let Some(cast) = cast {
        let _ = cdp(session, cast.tab, "Page.stopScreencast", json!({})).await;
    }
}

/// Puts the pane's layout back on `tab` after something else sized it. The
/// agent's screenshot lays the page out at its own size and then clears the
/// override, which left the pane drawing a page reflowed to Chromium's
/// default until the reader resized it.
pub async fn restore_metrics(session: &str, tab: i32) {
    if let Some(cast) = cast_of(session).filter(|c| c.tab == tab) {
        let _ = apply_metrics(session, cast).await;
    }
}

/// Moves a running screencast onto the session's active tab, where the pane
/// is now looking. A session with none is left alone.
async fn recast(session: &str) {
    let Some(cast) = cast_of(session) else { return };
    let Some(tab) = active(session) else {
        uncast(session).await;
        return;
    };
    if cast.tab == tab {
        return;
    }
    let _ = cdp(session, cast.tab, "Page.stopScreencast", json!({})).await;
    let moved = Cast { tab, ..cast };
    CAST.lock().unwrap().get_or_insert_with(HashMap::new).insert(session.to_string(), moved);
    let _ = start_cast(session, moved).await;
}

async fn apply_metrics(session: &str, cast: Cast) -> Result<(), String> {
    cdp(
        session,
        cast.tab,
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": cast.width,
            "height": cast.height,
            "deviceScaleFactor": cast.scale,
            "mobile": cast.touch,
        }),
    )
    .await?;
    let _ = cdp(
        session,
        cast.tab,
        "Emulation.setTouchEmulationEnabled",
        json!({ "enabled": cast.touch, "maxTouchPoints": 2 }),
    )
    .await;
    Ok(())
}

async fn start_cast(session: &str, cast: Cast) -> Result<(), String> {
    apply_metrics(session, cast).await?;
    cdp(
        session,
        cast.tab,
        "Page.startScreencast",
        json!({
            "format": "jpeg",
            "quality": FRAME_QUALITY,
            "maxWidth": (cast.width as f64 * cast.scale).ceil() as u32,
            "maxHeight": (cast.height as f64 * cast.scale).ceil() as u32,
            "everyNthFrame": 1,
        }),
    )
    .await?;
    Ok(())
}

/// One input event from the pane onto the session's active tab, in CDP's own
/// shape: the pane already speaks device coordinates, so nothing is
/// translated here.
pub async fn input(session: &str, method: &str, params: Value) -> Result<(), String> {
    if !INPUT_METHODS.contains(&method) {
        return Err(format!("{method} is not an input event"));
    }
    let tab = active(session).ok_or("no tab is open in this session's browser")?;
    cdp(session, tab, method, params).await?;
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
            let changed = with_tabs(|tabs| {
                let Some(record) = tabs.values_mut().find(|r| r.target == target) else { return None };
                let before = (record.title.clone(), record.url.clone());
                if let Some(title) = info["title"].as_str() {
                    record.title = title.to_string();
                }
                if let Some(url) = info["url"].as_str() {
                    record.url = url.to_string();
                }
                (before != (record.title.clone(), record.url.clone())).then(|| record.session.clone())
            });
            if let Some(session) = changed {
                publish_tabs(&session);
            }
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
                    publish_tab(tab);
                }
                "Page.loadEventFired" => {
                    settle(tab, &connection, Some(false)).await;
                    publish_tab(tab);
                }
                // The pane's picture of the page. Acked straight after
                // emitting, since Chromium sends no next frame until the
                // last one is acknowledged — a slow ack is a slow pane, and
                // a missed one freezes it.
                "Page.screencastFrame" => {
                    if let Some(session) = session_of(tab) {
                        let meta = &params["metadata"];
                        super::emit(
                            "browser_frame",
                            json!({
                                "sessionId": session,
                                "tab": tab,
                                "data": params["data"],
                                "width": meta["deviceWidth"],
                                "height": meta["deviceHeight"],
                            }),
                        );
                    }
                    let _ = connection
                        .call("Page.screencastFrameAck", json!({ "sessionId": params["sessionId"] }))
                        .await;
                }
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
    if let Some(casts) = CAST.lock().unwrap().as_mut() {
        casts.remove(session);
    }
    publish_tabs(session);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn input_refuses_anything_but_an_input_method() {
        let err = input("nobody", "Page.navigate", json!({ "url": "http://x" })).await.unwrap_err();
        assert!(err.contains("not an input event"), "{err}");
        // An allowed method still needs a tab, which is the next refusal —
        // so the allowlist is what answered above, not the missing tab.
        let err = input("nobody", "Input.insertText", json!({ "text": "a" })).await.unwrap_err();
        assert!(err.contains("no tab"), "{err}");
    }

    /// Live: proves the shapes the pane sends are the ones Chromium takes.
    /// A screencast started through `cast` delivers a frame *and a second
    /// one after input*, which is the ack working; a touch tap lands as a
    /// click; a key and inserted text land in a focused field.
    /// `cargo test --lib browser::backend -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "starts a real browser"]
    async fn the_pane_drives_a_page() {
        let session = format!("pane-live-{}", std::process::id());
        let page = "data:text/html,<button id=b onclick=\"document.title='tapped'\" style=\"position:fixed;left:0;top:0;width:100px;height:100px\">go</button><input id=i autofocus>";
        open(&session, page.to_string(), true).await.expect("open");
        let tab = active(&session).expect("a tab");
        let instance = super::super::instance(&session).await.unwrap();
        let connection = instance.page(tab).await.unwrap().connection.clone();
        let mut events = connection.subscribe();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        cast(&session, 400, 700, 2.0, true).await.expect("cast");
        async fn frame(events: &mut tokio::sync::broadcast::Receiver<Value>) -> Value {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    let e = events.recv().await.expect("stream closed");
                    if e["method"] == "Page.screencastFrame" {
                        let data = e["params"]["data"].as_str().unwrap_or_default();
                        use base64::Engine;
                        let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default();
                        // JPEG SOF0/SOF2 marker carries height then width, big-endian.
                        let mut i = 2;
                        while i + 9 < bytes.len() {
                            if bytes[i] == 0xFF && (bytes[i + 1] == 0xC0 || bytes[i + 1] == 0xC2) {
                                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
                                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
                                println!("jpeg pixels: {w}x{h}, {} bytes", bytes.len());
                                break;
                            }
                            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                            i += 2 + len;
                        }
                        return e["params"]["metadata"].clone();
                    }
                }
            })
            .await
            .expect("no frame")
        }
        let first = frame(&mut events).await;
        assert_eq!(first["deviceWidth"], 400, "laid out at the pane's width: {first}");
        println!("frame metadata: {first}");

        input(&session, "Input.dispatchTouchEvent", json!({ "type": "touchStart", "touchPoints": [{ "x": 50, "y": 50 }] }))
            .await
            .expect("touchStart");
        input(&session, "Input.dispatchTouchEvent", json!({ "type": "touchEnd", "touchPoints": [] }))
            .await
            .expect("touchEnd");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let title = connection
            .call("Runtime.evaluate", json!({ "expression": "document.title", "returnByValue": true }))
            .await
            .unwrap();
        assert_eq!(title["result"]["value"], "tapped", "a touch tap is a click");
        let _ = frame(&mut events).await;

        connection
            .call("Runtime.evaluate", json!({ "expression": "document.getElementById('i').focus()" }))
            .await
            .unwrap();
        input(&session, "Input.insertText", json!({ "text": "hi" })).await.expect("insertText");
        input(
            &session,
            "Input.dispatchKeyEvent",
            json!({ "type": "keyDown", "key": "!", "code": "Digit1", "text": "!", "unmodifiedText": "!", "windowsVirtualKeyCode": 49, "modifiers": 8 }),
        )
        .await
        .expect("keyDown");
        input(&session, "Input.dispatchKeyEvent", json!({ "type": "keyUp", "key": "!", "code": "Digit1", "windowsVirtualKeyCode": 49, "modifiers": 8 }))
            .await
            .expect("keyUp");
        let value = connection
            .call("Runtime.evaluate", json!({ "expression": "document.getElementById('i').value", "returnByValue": true }))
            .await
            .unwrap();
        assert_eq!(value["result"]["value"], "hi!", "typed text lands in the field");

        // The agent's screenshot asks for one pixel per CSS pixel under the
        // forced compositor scale; if the PNG comes back doubled, `capture`
        // in automation.rs has to counter it.
        cdp(&session, tab, "Emulation.setDeviceMetricsOverride", json!({ "width": 300, "height": 200, "deviceScaleFactor": 1, "mobile": false }))
            .await
            .unwrap();
        let shot = cdp(&session, tab, "Page.captureScreenshot", json!({ "format": "png" })).await.unwrap();
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.decode(shot["data"].as_str().unwrap()).unwrap();
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        println!("screenshot pixels: {w} wide for a 300 css layout");
        assert_eq!(w, 300, "a screenshot at deviceScaleFactor 1 is one pixel per CSS pixel");

        uncast(&session).await;
        super::super::close(&session).await;
        let _ = std::fs::remove_dir_all(std::env::home_dir().unwrap().join(".dray/browser").join(&session));
    }

    #[test]
    fn tab_info_crosses_in_camel_case() {
        let info = TabInfo { id: 1, url: "u".into(), title: "t".into(), loading: true, active: false };
        let json = serde_json::to_value(info).unwrap();
        assert_eq!(json["loading"], true);
        assert_eq!(json["active"], false);
    }
}
