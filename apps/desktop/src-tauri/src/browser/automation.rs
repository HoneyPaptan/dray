//! Driving a session's tabs for `dray browser`: the agent's half of the
//! in-app browser, with agent-browser's verbs.
//!
//! No agent-browser and no debug port of its own. Every action is a few CDP
//! calls on the session's active tab, and an agent can reach no other
//! session's pages because the session is the only address there is. Pointer
//! and key actions go through Chromium's real input path (`Input.dispatch*`)
//! rather than `element.click()`, so what the agent does is what a person's
//! click does; reads and locators are page JavaScript.
//!
//! **Which browser carries the message is one import.** `be` is
//! [`super::backend`], which opens, closes and watches tabs on a Chromium
//! Dray starts; every verb below is written against CDP alone and knows
//! nothing else about it.

use super::backend as be;

use base64::Engine;
use dray_proto::{BrowserAction, Get, Is, Locator};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

type Reply = Result<Value, String>;
/// Text for the agent, and the same answer as JSON.
type Answer = Result<(String, Value), String>;

/// The size `set viewport`/`set device` asked for, per session. Only
/// `screenshot` reads it: the widget stays the pane's size and the page is
/// laid out at this size for the capture alone.
///
/// It used to ride an event into the pane's own viewport store as well, so
/// the device bar would show what the agent set — and that store is what
/// puts the stage into device-preview layout, centred and padded inside the
/// pane. So an agent sizing one capture left the reader's page letterboxed
/// for the rest of the session, with the bar that explains it closed and
/// nothing clearing it (DRA-233). The override below is what sizes a
/// capture; shrinking the widget was never part of it.
static VIEWPORT: Mutex<Option<HashMap<String, (u32, u32)>>> = Mutex::new(None);
/// A laptop, since nearly everything looked at through here is a page built
/// for one; the pane itself is a third of a window and lays a page out at
/// phone breakpoints.
const DEFAULT_VIEWPORT: (u32, u32) = (1440, 900);
/// One repaint's worth of time, spent after the metrics override goes on:
/// a page that reflows paints a frame or two later, and capturing inside
/// that window catches the layout half-moved. The backends spend it at the
/// other two edges of a shot, where what is being waited for is a widget.
const SETTLE: Duration = Duration::from_millis(150);
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Snapshots and page text are for a model to read; past this they cost more
/// than they say.
const MAX_TEXT: usize = 40_000;

/// The pane's device presets, by name. Duplicated from `VIEWPORT_PRESETS`
/// in browser.ts, since the refusal for an unknown name has to come from
/// here; a test holds the two together.
const DEVICES: &[(&str, u32, u32)] = &[
    ("iPhone SE", 375, 667),
    ("iPhone 15", 393, 852),
    ("Pixel 8", 412, 915),
    ("iPad Mini", 768, 1024),
    ("iPad Air", 820, 1180),
    ("Laptop", 1280, 800),
    ("Desktop", 1440, 900),
];

/// Runs `expression` in the page and answers its value. A thrown error is
/// the error.
async fn eval(session: &str, tab: i32, expression: &str) -> Reply {
    let reply = be::cdp(
        session,
        tab,
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
    )
    .await?;
    if let Some(exception) = reply.get("exceptionDetails") {
        let text = exception
            .pointer("/exception/description")
            .or_else(|| exception.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("the script threw");
        return Err(text.lines().next().unwrap_or(text).to_string());
    }
    Ok(reply.pointer("/result/value").cloned().unwrap_or(Value::Null))
}

/// Runs `body` with `el` bound to the located element, or fails naming what
/// was looked for.
async fn with_element(session: &str, tab: i32, at: &Locator, body: &str) -> Reply {
    let js = format!(
        "(() => {{ {HELPERS_JS} const el = __find({})[0]; if (!el) return {{ __missing: true }}; {body} }})()",
        serde_json::to_string(at).unwrap()
    );
    let value = eval(session, tab, &js).await?;
    if value.get("__missing").is_some() {
        return Err(format!("nothing matches {}", describe_locator(at)));
    }
    Ok(value)
}

fn describe_locator(at: &Locator) -> String {
    match at {
        Locator::Target { target } => target.clone(),
        Locator::Role { role, name: Some(n), .. } => format!("{role} \"{n}\""),
        Locator::Role { role, .. } => role.clone(),
        Locator::Text { text, .. } => format!("text \"{text}\""),
        Locator::Label { label, .. } => format!("label \"{label}\""),
        Locator::Placeholder { placeholder, .. } => format!("placeholder \"{placeholder}\""),
        Locator::Alt { alt, .. } => format!("alt \"{alt}\""),
        Locator::Title { title, .. } => format!("title \"{title}\""),
        Locator::TestId { id } => format!("testid {id}"),
        Locator::Nth { selector, index } => format!("{selector}[{index}]"),
    }
}

/// Waits for the tab's load to settle. A navigation takes a moment to
/// start — a click's reply lands before the renderer has begun leaving the
/// page — so this first watches for loading to *begin*, up to a short
/// window, or "not loading" is answered before the previous page has even
/// been left and the next command reads the old URL. Still loading at the
/// deadline is an error, not a success with a half-loaded page behind it.
async fn wait_loaded(tab: i32) -> Result<(), String> {
    let start = Instant::now();
    let mut seen_loading = false;
    while start.elapsed() < LOAD_TIMEOUT {
        match be::tab_state(tab) {
            Some((_, _, true)) => seen_loading = true,
            Some(_) if seen_loading || start.elapsed() > Duration::from_millis(600) => return Ok(()),
            Some(_) => {}
            None => return Ok(()),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("the page is still loading after {}s", LOAD_TIMEOUT.as_secs()))
}

/// `dray browser` opens web pages. `file://` would hand an agent every file
/// the app can read, through `get text`; the other schemes are Chromium's
/// own. Judged on the *parsed* scheme, with the same WHATWG parser Chromium
/// applies, since that parser strips tabs and newlines and a hand-rolled
/// prefix check would pass `fi\tle://` as no scheme at all.
fn web_url(url: &str) -> Result<(), String> {
    if url.trim() == "about:blank" {
        return Ok(());
    }
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("{url:?} is not a URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(format!("only http and https pages can be opened from here, not {scheme}:")),
    }
}

/// A tab id the caller may act on: one of this session's.
fn owned(session: &str, id: i32) -> Result<i32, String> {
    if be::tabs(session).iter().any(|t| t.id == id) {
        Ok(id)
    } else {
        Err(format!("no tab {id} in this session; `dray browser tab` lists them"))
    }
}

fn active_tab(session: &str) -> Result<i32, String> {
    be::active(session).ok_or_else(|| {
        "no tab is open in this session's browser; `dray browser open <url>` first".to_string()
    })
}

fn page(tab: i32) -> (String, Value) {
    match be::tab_state(tab) {
        Some((url, title, _)) => {
            let text = if title.is_empty() { url.clone() } else { format!("{title} — {url}") };
            (text, json!({ "url": url, "title": title }))
        }
        None => ("no tab".into(), json!({})),
    }
}

/// Waits for a tab that was not there when `before` was read.
async fn new_tab(session: &str, before: &[i32]) -> Result<i32, String> {
    let start = Instant::now();
    loop {
        if let Some(id) = be::tabs(session).iter().map(|t| t.id).find(|id| !before.contains(id)) {
            return Ok(id);
        }
        if start.elapsed() > LOAD_TIMEOUT {
            return Err("Chromium did not open a tab".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn mouse(session: &str, tab: i32, kind: &str, x: f64, y: f64, extra: Value) -> Result<(), String> {
    let mut params = json!({ "type": kind, "x": x, "y": y });
    params.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    be::cdp(session, tab, "Input.dispatchMouseEvent", params).await.map(|_| ())
}

/// The viewport centre of the element, scrolled into view first so a click
/// lands on it rather than on whatever covers an off-screen point — and
/// refused where something else *does* cover that point, since the click
/// would land on the cover and report success.
async fn center(session: &str, tab: i32, at: &Locator) -> Result<(f64, f64), String> {
    let point = with_element(
        session,
        tab,
        at,
        "if (!__visible(el)) return { hidden: true }; \
         el.scrollIntoView({ block: 'center', inline: 'center' }); \
         const r = el.getBoundingClientRect(); \
         const x = r.left + r.width / 2, y = r.top + r.height / 2; \
         const hit = document.elementFromPoint(x, y); \
         if (!hit || !el.contains(hit)) \
           return { covered: hit ? hit.tagName.toLowerCase() + (hit.id ? '#' + hit.id : '') : 'nothing' }; \
         return { x, y };",
    )
    .await?;
    if point.get("hidden").is_some() {
        return Err(format!("{} is not visible", describe_locator(at)));
    }
    if let Some(cover) = point.get("covered").and_then(Value::as_str) {
        return Err(format!("{} is covered by <{cover}>", describe_locator(at)));
    }
    Ok((point["x"].as_f64().unwrap_or(0.0), point["y"].as_f64().unwrap_or(0.0)))
}

async fn click(session: &str, tab: i32, at: &Locator, count: u32) -> Result<(), String> {
    let (x, y) = center(session, tab, at).await?;
    mouse(session, tab, "mouseMoved", x, y, json!({})).await?;
    for n in 1..=count {
        let button = json!({ "button": "left", "clickCount": n });
        mouse(session, tab, "mousePressed", x, y, button.clone()).await?;
        mouse(session, tab, "mouseReleased", x, y, button).await?;
    }
    Ok(())
}

async fn focus(session: &str, tab: i32, at: &Locator, clear: bool) -> Result<(), String> {
    let body = format!(
        "el.focus(); if ({clear}) {{ \
           if (el.isContentEditable) el.textContent = ''; \
           else if ('value' in el) {{ el.value = ''; el.dispatchEvent(new Event('input', {{ bubbles: true }})); }} \
         }} return true;"
    );
    with_element(session, tab, at, &body).await.map(|_| ())
}

/// The same reading `is checked` takes, so an ARIA switch is toggled rather
/// than reported already there.
const CHECKED_JS: &str = "if (!/^(checkbox|radio)$/.test(el.type || '') && !/^(checkbox|radio|switch|menuitemcheckbox)$/.test(el.getAttribute('role') || '')) \
       return { notCheckable: true }; \
     return !!el.checked || el.getAttribute('aria-checked') === 'true';";

async fn checked(session: &str, tab: i32, at: &Locator) -> Result<bool, String> {
    let now = with_element(session, tab, at, CHECKED_JS).await?;
    if now.get("notCheckable").is_some() {
        return Err(format!("{} is not a checkbox", describe_locator(at)));
    }
    Ok(now == Value::Bool(true))
}

async fn set_checked(session: &str, tab: i32, at: &Locator, on: bool) -> Result<(), String> {
    if checked(session, tab, at).await? == on {
        return Ok(());
    }
    click(session, tab, at, 1).await?;
    // A radio cannot be unchecked by clicking, and a custom control may
    // ignore the click; read it back rather than report the click as done.
    if checked(session, tab, at).await? != on {
        return Err(format!("{} did not change when clicked", describe_locator(at)));
    }
    Ok(())
}

/// The whole of `dray browser`: one action on the session's active tab.
///
/// An input verb first brings the tab's view out of hiding, off-screen —
/// see `reveal` — and the layout is put back after, whatever the outcome.
pub async fn run(session: &str, action: BrowserAction) -> Answer {
    let input = matches!(
        action,
        BrowserAction::Click { .. }
            | BrowserAction::DblClick { .. }
            | BrowserAction::Hover { .. }
            | BrowserAction::Type { .. }
            | BrowserAction::Fill { .. }
            | BrowserAction::Press { .. }
            | BrowserAction::Check { .. }
            | BrowserAction::Uncheck { .. }
    );
    let tab = be::active(session);
    if let (true, Some(tab)) = (input, tab) {
        be::before_input(tab).await?;
    }
    let answer = perform(session, action).await;
    if input {
        be::after_input();
    }
    answer
}

async fn perform(session: &str, action: BrowserAction) -> Answer {
    let ok = |text: String| Ok((text, json!({ "ok": true })));
    let clear = matches!(action, BrowserAction::Fill { .. });
    match action {
        BrowserAction::Open { url } => {
            web_url(&url)?;
            let tab = match be::active(session) {
                Some(tab) => {
                    be::open(session, url, false).await?;
                    tab
                }
                None => {
                    let before: Vec<i32> = be::tabs(session).iter().map(|t| t.id).collect();
                    be::open(session, url, true).await?;
                    new_tab(session, &before).await?
                }
            };
            wait_loaded(tab).await?;
            Ok(page(tab))
        }
        BrowserAction::Back | BrowserAction::Forward | BrowserAction::Reload => {
            let tab = active_tab(session)?;
            let verb = match action {
                BrowserAction::Back => "back",
                BrowserAction::Forward => "forward",
                _ => "reload",
            };
            be::nav(session, verb).await?;
            wait_loaded(tab).await?;
            Ok(page(tab))
        }
        BrowserAction::Close => {
            let tab = active_tab(session)?;
            be::close_tab(session, tab).await?;
            ok(format!("closed tab {tab}"))
        }
        BrowserAction::Tabs => {
            let tabs = be::tabs(session);
            let text = if tabs.is_empty() {
                "no tabs".to_string()
            } else {
                tabs.iter()
                    .map(|t| format!("{} {}{}", t.id, if t.active { "* " } else { "  " }, page(t.id).0))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let data = tabs
                .iter()
                .map(|t| json!({ "id": t.id, "active": t.active, "url": t.url, "title": t.title }))
                .collect();
            Ok((text, Value::Array(data)))
        }
        BrowserAction::TabNew { url } => {
            let url = url.unwrap_or_else(|| "about:blank".into());
            web_url(&url)?;
            let before: Vec<i32> = be::tabs(session).iter().map(|t| t.id).collect();
            be::open(session, url, true).await?;
            let tab = new_tab(session, &before).await?;
            wait_loaded(tab).await?;
            let (text, mut data) = page(tab);
            data["id"] = json!(tab);
            Ok((format!("{tab} {text}"), data))
        }
        BrowserAction::TabSwitch { id } => {
            let id = owned(session, id)?;
            be::activate(session, id).await?;
            Ok(page(id))
        }
        BrowserAction::TabClose { id } => {
            let id = match id {
                Some(id) => owned(session, id)?,
                None => active_tab(session)?,
            };
            be::close_tab(session, id).await?;
            ok(format!("closed tab {id}"))
        }
        BrowserAction::Snapshot { interactive, compact, selector } => {
            let tab = active_tab(session)?;
            let opts = json!({ "interactive": interactive, "compact": compact, "selector": selector });
            let text = eval(session, tab, &format!("(() => {{ {HELPERS_JS} return __snapshot({opts}); }})()")).await?;
            let text = clip(text.as_str().unwrap_or(""));
            Ok((text.clone(), json!({ "snapshot": text })))
        }
        BrowserAction::Click { at } => {
            let tab = active_tab(session)?;
            click(session, tab, &at, 1).await?;
            wait_loaded(tab).await?;
            ok(format!("clicked {}", describe_locator(&at)))
        }
        BrowserAction::DblClick { at } => {
            let tab = active_tab(session)?;
            click(session, tab, &at, 2).await?;
            ok(format!("double-clicked {}", describe_locator(&at)))
        }
        BrowserAction::Focus { at } => {
            focus(session, active_tab(session)?, &at, false).await?;
            ok(format!("focused {}", describe_locator(&at)))
        }
        BrowserAction::Hover { at } => {
            let tab = active_tab(session)?;
            let (x, y) = center(session, tab, &at).await?;
            mouse(session, tab, "mouseMoved", x, y, json!({})).await?;
            ok(format!("hovering {}", describe_locator(&at)))
        }
        BrowserAction::Type { at, text } | BrowserAction::Fill { at, text } => {
            let tab = active_tab(session)?;
            focus(session, tab, &at, clear).await?;
            be::cdp(session, tab, "Input.insertText", json!({ "text": text })).await?;
            ok(format!("{} {}", if clear { "filled" } else { "typed into" }, describe_locator(&at)))
        }
        BrowserAction::Press { key } => {
            let tab = active_tab(session)?;
            let (down, up) = key_events(&key)?;
            be::cdp(session, tab, "Input.dispatchKeyEvent", down).await?;
            be::cdp(session, tab, "Input.dispatchKeyEvent", up).await?;
            wait_loaded(tab).await?;
            ok(format!("pressed {key}"))
        }
        BrowserAction::Check { at } => {
            set_checked(session, active_tab(session)?, &at, true).await?;
            ok(format!("checked {}", describe_locator(&at)))
        }
        BrowserAction::Uncheck { at } => {
            set_checked(session, active_tab(session)?, &at, false).await?;
            ok(format!("unchecked {}", describe_locator(&at)))
        }
        BrowserAction::Select { at, value } => {
            let tab = active_tab(session)?;
            let body = format!(
                "const want = {}; const opt = [...el.options || []].find(o => o.value === want || o.label.trim() === want); \
                 if (!opt) return {{ found: false }}; el.value = opt.value; \
                 el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); \
                 return {{ found: true, value: opt.value }};",
                Value::String(value.clone())
            );
            let reply = with_element(session, tab, &at, &body).await?;
            if reply["found"] != Value::Bool(true) {
                return Err(format!("{} has no option {value:?}", describe_locator(&at)));
            }
            ok(format!("selected {value} in {}", describe_locator(&at)))
        }
        BrowserAction::Scroll { direction, amount } => {
            let tab = active_tab(session)?;
            let (dx, dy) = match direction.as_str() {
                "up" => (0.0, -amount),
                "down" => (0.0, amount),
                "left" => (-amount, 0.0),
                "right" => (amount, 0.0),
                other => return Err(format!("scroll up, down, left or right — not {other}")),
            };
            // `Input.dispatchMouseEvent` of `mouseWheel` never answers on a
            // page with nothing to scroll; the page's own API always does.
            eval(session, tab, &format!("window.scrollBy({dx}, {dy}); window.scrollY")).await?;
            ok(format!("scrolled {direction} {amount}"))
        }
        BrowserAction::ScrollIntoView { at } => {
            with_element(session, active_tab(session)?, &at, "el.scrollIntoView({ block: 'center' }); return true;").await?;
            ok(format!("scrolled to {}", describe_locator(&at)))
        }
        BrowserAction::Get { what, at } => {
            let tab = active_tab(session)?;
            let at = at.unwrap_or(Locator::Target { target: "html".into() });
            let value = match what {
                Get::Title => page(tab).1["title"].clone(),
                Get::Url => page(tab).1["url"].clone(),
                Get::Text => with_element(session, tab, &at, "return el.innerText ?? el.textContent ?? '';").await?,
                Get::Html => with_element(session, tab, &at, "return el.outerHTML;").await?,
                Get::Value => with_element(session, tab, &at, "return el.value ?? null;").await?,
                Get::Attr { name } => {
                    let body = format!("return el.getAttribute({});", Value::String(name));
                    with_element(session, tab, &at, &body).await?
                }
                Get::Box => {
                    with_element(
                        session,
                        tab,
                        &at,
                        "const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height };",
                    )
                    .await?
                }
                Get::Count => {
                    let js = format!(
                        "(() => {{ {HELPERS_JS} return __find({}).length; }})()",
                        serde_json::to_string(&at).unwrap()
                    );
                    eval(session, tab, &js).await?
                }
            };
            let text = match &value {
                Value::String(s) => clip(s),
                Value::Null => "null".into(),
                other => other.to_string(),
            };
            Ok((text, json!({ "value": value })))
        }
        BrowserAction::Is { what, at } => {
            let tab = active_tab(session)?;
            let body = match what {
                Is::Visible => "return __visible(el);",
                Is::Enabled => "return !el.disabled && el.getAttribute('aria-disabled') !== 'true';",
                Is::Checked => "return !!el.checked || el.getAttribute('aria-checked') === 'true';",
            };
            // No match is `false`; a broken selector or a dead tab is an
            // error, or a typo reads as page state.
            let value = match with_element(session, tab, &at, body).await {
                Ok(value) => value,
                Err(e) if e.starts_with("nothing matches") => Value::Bool(false),
                Err(e) => return Err(e),
            };
            let yes = value == Value::Bool(true);
            Ok((yes.to_string(), json!({ "value": yes })))
        }
        BrowserAction::Wait { selector, ms, url, text, load } => {
            let tab = active_tab(session)?;
            if let Some(ms) = ms {
                tokio::time::sleep(Duration::from_millis(ms.min(60_000))).await;
                return ok(format!("waited {ms}ms"));
            }
            if let Some(state) = load {
                if state != "load" {
                    return Err(format!("wait --load takes `load`; nothing here measures {state}"));
                }
                wait_loaded(tab).await?;
                return ok(format!("loaded: {}", page(tab).0));
            }
            let (probe, what) = if let Some(sel) = selector {
                let at = serde_json::to_string(&Locator::Target { target: sel.clone() }).unwrap();
                (format!("(() => {{ {HELPERS_JS} return __find({at}).length > 0; }})()"), sel)
            } else if let Some(url) = url {
                (format!("location.href.includes({})", Value::String(url.clone())), url)
            } else if let Some(text) = text {
                (format!("(document.body?.innerText || '').includes({})", Value::String(text.clone())), text)
            } else {
                return Err("wait for what? a selector, --url, --text, --load or a number of ms".into());
            };
            let start = Instant::now();
            while start.elapsed() < LOAD_TIMEOUT {
                if eval(session, tab, &probe).await.unwrap_or(Value::Bool(false)) == Value::Bool(true) {
                    return ok(format!("{what} is there"));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(format!("{what} did not appear within {}s", LOAD_TIMEOUT.as_secs()))
        }
        BrowserAction::Screenshot { path, full } => {
            let tab = active_tab(session)?;
            let (w, h) = screenshot_size(session);
            let held = be::capture_guard().await;
            // The pane draws the page's own still and hides the view behind
            // it, so the reflow the capture needs happens off screen. A
            // backend with no widget on screen answers at once.
            let shot = be::cover(session).await;
            let bytes = capture(session, tab, w, h, full).await;
            // Cleared on the failing path too, or one timed-out capture leaves
            // the tab laid out at a width nobody asked for and every later
            // verb reads a page that isn't the one on screen. The shutter
            // closes there for the same reason: a capture that failed must
            // not leave the pane holding the camera card for good.
            let _ = be::cdp(session, tab, "Emulation.clearDeviceMetricsOverride", json!({})).await;
            // The pane's own layout goes back on, or the page it is drawing
            // stays reflowed to Chromium's default until the reader resizes.
            be::restore_metrics(session, tab).await;
            be::uncover(session, shot).await;
            drop(held);
            let bytes = bytes?;
            let path = match path {
                Some(p) => screenshot_path(session, &p).await?,
                None => {
                    let dir = be::shots_dir();
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    dir.join(format!("{}-{stamp}.png", &session[..8.min(session.len())]))
                }
            };
            write_nofollow(&path, &bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
            let shown = path.display().to_string();
            Ok((shown.clone(), json!({ "path": shown })))
        }
        BrowserAction::Eval { js } => {
            let value = eval(session, active_tab(session)?, &js).await?;
            let text = match &value {
                Value::String(s) => s.clone(),
                Value::Null => "undefined".into(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            };
            Ok((text, json!({ "value": value })))
        }
        BrowserAction::Console | BrowserAction::Errors => {
            let tab = active_tab(session)?;
            let errors_only = matches!(action, BrowserAction::Errors);
            let lines: Vec<(bool, String)> = be::console_drain(tab)
                .into_iter()
                .filter(|(error, _)| *error || !errors_only)
                .collect();
            let text = if lines.is_empty() {
                if errors_only { "no errors" } else { "nothing logged" }.to_string()
            } else {
                lines
                    .iter()
                    .map(|(e, t)| format!("{} {t}", if *e { "[error]" } else { "[log]" }))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let data = lines
                .into_iter()
                .map(|(e, t)| json!({ "level": if e { "error" } else { "log" }, "text": t }))
                .collect();
            Ok((text, Value::Array(data)))
        }
        BrowserAction::SetViewport { width, height } => {
            active_tab(session)?;
            remember_viewport(session, width, height);
            ok(format!("viewport {width}×{height}"))
        }
        BrowserAction::SetDevice { name } => {
            active_tab(session)?;
            let (label, w, h) = DEVICES
                .iter()
                .find(|(n, _, _)| n.eq_ignore_ascii_case(&name))
                .ok_or_else(|| {
                    let names = DEVICES.iter().map(|d| d.0).collect::<Vec<_>>().join(", ");
                    format!("no device {name:?}; one of {names}")
                })?;
            remember_viewport(session, *w, *h);
            ok(format!("{label} {w}×{h}"))
        }
    }
}

fn remember_viewport(session: &str, width: u32, height: u32) {
    VIEWPORT.lock().unwrap().get_or_insert_with(HashMap::new).insert(session.into(), (width, height));
}

/// What `screenshot` lays the page out at: the session's own pick, else a laptop.
fn screenshot_size(session: &str) -> (u32, u32) {
    VIEWPORT
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(session).copied())
        .unwrap_or(DEFAULT_VIEWPORT)
}

/// The PNG of the page laid out at `w`×`h`. The widget is the pane's size,
/// so `Page.captureScreenshot` alone answers a desktop layout as a phone; the
/// metrics override is the one thing that sizes a page independently of the
/// widget drawing it. `deviceScaleFactor: 1`, or a 1440-wide request answers
/// a 2880-wide PNG on a retina screen. The caller clears the override.
async fn capture(session: &str, tab: i32, w: u32, h: u32, full: bool) -> Result<Vec<u8>, String> {
    be::cdp(
        session,
        tab,
        "Emulation.setDeviceMetricsOverride",
        json!({ "width": w, "height": h, "deviceScaleFactor": 1, "mobile": false }),
    )
    .await?;
    // A page that reflows paints a frame or two later; capturing inside that
    // window catches the layout half-moved.
    tokio::time::sleep(SETTLE).await;
    let mut params = json!({ "format": "png" });
    if full {
        let size = eval(session, tab, "({ w: document.documentElement.scrollWidth, h: document.documentElement.scrollHeight })").await?;
        params["captureBeyondViewport"] = json!(true);
        params["clip"] = json!({ "x": 0, "y": 0, "width": size["w"], "height": size["h"], "scale": 1 });
    }
    let reply = be::cdp(session, tab, "Page.captureScreenshot", params).await?;
    let data = reply["data"].as_str().ok_or("no image came back")?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("bad image data: {e}"))
}

/// Truncating write that refuses a symlink at the leaf, so a link planted
/// between `screenshot_path`'s check and this open cannot point the write
/// elsewhere. The parent was canonicalized a moment earlier; a parent
/// swapped in that window is the one race left, and it needs write access to
/// the checkout, which is the agent's own.
fn write_nofollow(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(bytes)
}

/// A caller's screenshot path, admitted only under the session's own
/// checkout. `dray` runs with no consent card and this write truncates, so
/// an open path would let a page-steered agent overwrite any file the app
/// can; the parent is canonicalized so a symlink cannot point back out.
async fn screenshot_path(session: &str, given: &str) -> Result<PathBuf, String> {
    let cwd = crate::store::get_session_index_item(session)
        .await
        .ok()
        .flatten()
        .map(|item| PathBuf::from(item.cwd))
        .ok_or("this session has no checkout to write under")?;
    let cwd = std::fs::canonicalize(&cwd).map_err(|e| format!("{}: {e}", cwd.display()))?;
    let full = if Path::new(given).is_absolute() { PathBuf::from(given) } else { cwd.join(given) };
    let name = full.file_name().ok_or("the path names no file")?.to_owned();
    let parent = full.parent().ok_or("the path names no directory")?;
    let parent = std::fs::canonicalize(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    if !parent.starts_with(&cwd) {
        return Err(format!(
            "screenshots go under the session's checkout ({}); with no path, under ~/.dray/browser/shots",
            cwd.display()
        ));
    }
    let path = parent.join(name);
    if path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(format!("{} is a symlink", path.display()));
    }
    Ok(path)
}

/// `keyDown`/`keyUp` for a key name with optional modifier prefixes. Named
/// keys carry their virtual key code, which is what a page's key handling
/// reads; a printable key carries its text, which is what an input reads.
fn key_events(spec: &str) -> Result<(Value, Value), String> {
    let mut modifiers = 0;
    let mut key = spec;
    while let Some((prefix, rest)) = key.split_once('+').filter(|(_, r)| !r.is_empty()) {
        modifiers |= match prefix.to_ascii_lowercase().as_str() {
            "alt" | "option" => 1,
            "ctrl" | "control" => 2,
            "meta" | "cmd" | "command" => 4,
            "shift" => 8,
            _ => return Err(format!("unknown modifier {prefix}")),
        };
        key = rest;
    }
    let (name, code, vk, text): (&str, String, i32, Option<String>) = match key {
        "Enter" | "Return" => ("Enter", "Enter".into(), 13, Some("\r".into())),
        "Tab" => ("Tab", "Tab".into(), 9, None),
        "Escape" | "Esc" => ("Escape", "Escape".into(), 27, None),
        "Backspace" => ("Backspace", "Backspace".into(), 8, None),
        "Delete" => ("Delete", "Delete".into(), 46, None),
        "Space" => (" ", "Space".into(), 32, Some(" ".into())),
        "ArrowUp" | "Up" => ("ArrowUp", "ArrowUp".into(), 38, None),
        "ArrowDown" | "Down" => ("ArrowDown", "ArrowDown".into(), 40, None),
        "ArrowLeft" | "Left" => ("ArrowLeft", "ArrowLeft".into(), 37, None),
        "ArrowRight" | "Right" => ("ArrowRight", "ArrowRight".into(), 39, None),
        "Home" => ("Home", "Home".into(), 36, None),
        "End" => ("End", "End".into(), 35, None),
        "PageUp" => ("PageUp", "PageUp".into(), 33, None),
        "PageDown" => ("PageDown", "PageDown".into(), 34, None),
        k if k.chars().count() == 1 => {
            let c = k.chars().next().unwrap();
            let code = if c.is_ascii_alphabetic() {
                format!("Key{}", c.to_ascii_uppercase())
            } else if c.is_ascii_digit() {
                format!("Digit{c}")
            } else {
                String::new()
            };
            let vk = c.to_ascii_uppercase() as i32;
            // A chord is a command, not text: ⌘A must select all, not type "a".
            let text = (modifiers & !8 == 0).then(|| c.to_string());
            (k, code, vk, text)
        }
        other => return Err(format!("unknown key {other}")),
    };
    let base = json!({ "key": name, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "modifiers": modifiers });
    let mut down = base.clone();
    down["type"] = json!(if text.is_some() { "keyDown" } else { "rawKeyDown" });
    if let Some(text) = text {
        // Both, as Puppeteer sends them: `text` alone types but does not
        // submit a form on Enter.
        down["text"] = json!(text);
        down["unmodifiedText"] = json!(text);
    }
    let mut up = base;
    up["type"] = json!("keyUp");
    Ok((down, up))
}

fn clip(text: &str) -> String {
    if text.len() <= MAX_TEXT {
        return text.to_string();
    }
    let cut = text.char_indices().map(|(i, _)| i).take_while(|&i| i <= MAX_TEXT).last().unwrap_or(0);
    format!("{}\n… [{} more characters]", &text[..cut], text.len() - cut)
}

/// Page-side helpers every script above opens with: what an element is
/// called and what it is, one locator over every `Locator` shape, and the
/// snapshot. One copy, so `find role button --name Submit` names exactly the
/// element `snapshot` would list as `button "Submit"`.
const HELPERS_JS: &str = r#"
  const __visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) return false;
    const cs = getComputedStyle(el);
    return cs.visibility !== 'hidden' && cs.display !== 'none';
  };
  const __text = (s) => (s || '').trim().replace(/\s+/g, ' ');
  const __name = (el) => __text(el.getAttribute('aria-label')
    || (el.labels && el.labels[0] && el.labels[0].innerText)
    || el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt')
    || el.innerText || el.textContent || el.getAttribute('name')).slice(0, 80);
  const __role = (el) => {
    const r = el.getAttribute('role');
    if (r) return r;
    const t = el.tagName.toLowerCase();
    if (t === 'a') return el.hasAttribute('href') ? 'link' : 'generic';
    if (t === 'button' || (t === 'input' && /^(button|submit|reset)$/.test(el.type))) return 'button';
    if (t === 'input') return el.type === 'checkbox' ? 'checkbox' : el.type === 'radio' ? 'radio' : 'textbox';
    if (t === 'textarea') return 'textbox';
    if (t === 'select') return 'combobox';
    if (t === 'option') return 'option';
    if (t === 'img') return 'img';
    if (/^h[1-6]$/.test(t)) return 'heading';
    if (t === 'li') return 'listitem';
    if (t === 'ul' || t === 'ol') return 'list';
    if (t === 'nav') return 'navigation';
    if (t === 'main') return 'main';
    if (t === 'form') return 'form';
    if (t === 'table') return 'table';
    if (el.isContentEditable) return 'textbox';
    return 'generic';
  };
  const __match = (have, want, exact) => {
    have = __text(have); want = __text(want);
    return exact ? have === want : have.toLowerCase().includes(want.toLowerCase());
  };
  const __attrMatch = (attr, want, exact) => [...document.querySelectorAll('[' + attr + ']')]
    .filter(el => __visible(el) && __match(el.getAttribute(attr), want, exact));
  const __find = (loc) => {
    switch (loc.by) {
      case 'target': {
        const sel = loc.target.startsWith('@') ? '[data-dray-ref="' + CSS.escape(loc.target.slice(1)) + '"]' : loc.target;
        return [...document.querySelectorAll(sel)];
      }
      case 'nth': {
        const all = [...document.querySelectorAll(loc.selector)];
        const el = all[loc.index < 0 ? all.length + loc.index : loc.index];
        return el ? [el] : [];
      }
      case 'role':
        return [...document.querySelectorAll('*')].filter(el => __visible(el) && __role(el) === loc.role
          && (loc.name == null || __match(__name(el), loc.name, loc.exact)));
      case 'text': {
        const hits = [...document.querySelectorAll('body *')].filter(el => __visible(el)
          && el.children.length < 20 && __match(el.innerText, loc.text, loc.exact));
        // The deepest match is the element the words belong to, not its ancestors.
        return hits.filter(el => !hits.some(o => o !== el && el.contains(o)));
      }
      case 'label': {
        const byLabel = [...document.querySelectorAll('label')]
          .filter(l => __match(l.innerText, loc.label, loc.exact))
          .map(l => l.control || (l.htmlFor && document.getElementById(l.htmlFor)))
          .filter(Boolean);
        return byLabel.length ? byLabel : __attrMatch('aria-label', loc.label, loc.exact);
      }
      case 'placeholder': return __attrMatch('placeholder', loc.placeholder, loc.exact);
      case 'alt': return __attrMatch('alt', loc.alt, loc.exact);
      case 'title': return __attrMatch('title', loc.title, loc.exact);
      case 'test_id': {
        const id = CSS.escape(loc.id);
        return [...document.querySelectorAll('[data-testid="' + id + '"], [data-test-id="' + id + '"]')];
      }
    }
    return [];
  };
  const __snapshot = (opts) => {
    document.querySelectorAll('[data-dray-ref]').forEach(el => el.removeAttribute('data-dray-ref'));
    const root = opts.selector ? document.querySelector(opts.selector) : document;
    if (!root) return 'nothing matches ' + opts.selector;
    const lines = [];
    let n = 0;
    const interactive = /^(link|button|textbox|checkbox|radio|combobox|option|tab|menuitem|switch|slider|searchbox)$/;
    for (const el of root.querySelectorAll('*')) {
      const r = __role(el);
      if (r === 'generic' || r === 'listitem' || r === 'list') continue;
      if (opts.interactive && !interactive.test(r)) continue;
      if (opts.compact && !(interactive.test(r) || r === 'heading')) continue;
      if (!__visible(el) || el.closest('[aria-hidden="true"]')) continue;
      if (n >= 400) { lines.push('… more elements not listed'); break; }
      const label = __name(el);
      if (!interactive.test(r)) { if (label) lines.push(r + ' "' + label + '"'); continue; }
      const ref = 'e' + (++n);
      el.setAttribute('data-dray-ref', ref);
      let line = '@' + ref + ' ' + r + ' "' + label + '"';
      if (r === 'link' && el.href) line += ' → ' + el.href;
      if ((r === 'textbox' || r === 'combobox') && 'value' in el && el.value) line += ' value="' + String(el.value).slice(0, 60) + '"';
      if (r === 'checkbox' || r === 'radio' || r === 'switch') line += (el.checked || el.getAttribute('aria-checked') === 'true') ? ' [checked]' : ' [ ]';
      if (el.disabled) line += ' [disabled]';
      lines.push(line);
    }
    return document.title + ' — ' + location.href + '\n' + lines.join('\n');
  };
"#;

/// The active tab as the pane sees it, for drawing in its place while the
/// native view is hidden under a modal. No metrics override: the picture
/// must match the widget's own size, or it is drawn stretched. The view
/// hides only once this lands, so the whole cost is a modal opening late
/// over the page: one CSS pixel per image pixel (a quarter of retina) and
/// a fast JPEG, since the picture lives as long as a menu is open. An
/// agent's sized screenshot holding `CAPTURING` would hand back a
/// phone-wide page, so this waits for it — briefly, since the pane gives
/// up on the answer at 400ms and a full-page capture can run for seconds;
/// past that the pane hides over nothing, as it did before. The tab is read
/// after the wait, so the picture is of the tab up when it is taken.
#[tauri::command]
pub async fn browser_snapshot(session_id: String) -> Result<String, String> {
    // Not while the shutter is open, and that exception is the whole reason
    // a shot can be covered by the page rather than by a blank. The lock is
    // held for the length of a shot, so a cover asked for inside one would
    // be refused — and the cover is what the shot hides behind. Safe
    // precisely there: the shutter opens *before* the override goes on, so
    // the page this reads is the one the reader is looking at.
    let _held = if be::shutter_is_open() { None } else { Some(be::try_capture_guard().await?) };
    let tab = active_tab(&session_id)?;
    // `innerWidth`, not the layout viewport's `clientWidth`: that one stops
    // at the scrollbar, and a picture a scrollbar short of the view is
    // stretched across it. The clip is in page coordinates, hence the
    // scroll offset from the metrics.
    let size = eval(&session_id, tab, "({ w: innerWidth, h: innerHeight })").await?;
    let metrics = be::cdp(&session_id, tab, "Page.getLayoutMetrics", json!({})).await?;
    let vp = &metrics["cssVisualViewport"];
    let clip = json!({
        "x": vp["pageX"], "y": vp["pageY"],
        "width": size["w"], "height": size["h"],
        "scale": 1,
    });
    let params = json!({ "format": "jpeg", "quality": 60, "optimizeForSpeed": true, "clip": clip });
    let reply = be::cdp(&session_id, tab, "Page.captureScreenshot", params).await?;
    let data = reply["data"].as_str().ok_or("no image came back")?;
    Ok(format!("data:image/jpeg;base64,{data}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locators_serialize_the_way_the_page_script_reads_them() {
        let js = serde_json::to_string(&Locator::Role { role: "button".into(), name: Some("Go".into()), exact: false }).unwrap();
        assert_eq!(js, r#"{"by":"role","role":"button","name":"Go","exact":false}"#);
        let js = serde_json::to_string(&Locator::TestId { id: "x".into() }).unwrap();
        assert!(js.contains(r#""by":"test_id""#), "the switch in HELPERS_JS spells it test_id");
    }

    #[test]
    fn chords_carry_no_text() {
        let (down, _) = key_events("Meta+a").unwrap();
        assert_eq!(down["modifiers"], 4);
        assert_eq!(down["type"], "rawKeyDown");
        let (down, _) = key_events("a").unwrap();
        assert_eq!(down["text"], "a");
        let (down, up) = key_events("Enter").unwrap();
        assert_eq!(down["windowsVirtualKeyCode"], 13);
        assert_eq!(up["type"], "keyUp");
    }

    #[test]
    fn screenshot_size_is_the_last_set_or_a_laptop() {
        assert_eq!(screenshot_size("unset"), (1440, 900));
        remember_viewport("s1", 375, 667);
        assert_eq!(screenshot_size("s1"), (375, 667));
        assert_eq!(screenshot_size("unset"), (1440, 900));
    }

    #[test]
    fn devices_match_the_pane_presets() {
        let ts = include_str!("../../../src/lib/browser.ts");
        for (name, w, h) in DEVICES {
            assert!(
                ts.contains(&format!("label: \"{name}\", width: {w}, height: {h}")),
                "{name} drifted from VIEWPORT_PRESETS"
            );
        }
    }
}

/// Live, and `#[ignore]`d for it: this starts a real browser and drives it
/// through the same `run` the socket calls.
///
/// The one test that proves a verb end to end rather than proving a type.
/// `about:blank` plus `eval` rather than a fixture page, so it needs no
/// server and no network — `web_url` refuses `data:` on purpose, and a
/// listener of our own would be a second thing that can fail.
/// `cargo test --lib browser::automation::live -- --ignored --nocapture`
#[cfg(test)]
mod live {
    use super::*;

    async fn eval_ok(session: &str, js: &str) -> Value {
        let (_, data) = run(session, BrowserAction::Eval { js: js.into() })
            .await
            .unwrap_or_else(|e| panic!("eval {js:?} failed: {e}"));
        data["value"].clone()
    }

    #[tokio::test]
    #[ignore = "starts a real browser"]
    async fn drives_a_page_through_the_verbs() {
        let session = format!("browser-live-{}", std::process::id());

        let (text, _) = run(&session, BrowserAction::Open { url: "about:blank".into() })
            .await
            .expect("could not open a tab");
        println!("open: {text}");

        // A button that records its own click, so the assertion below is
        // about Chromium's input path rather than about `element.click()`.
        eval_ok(
            &session,
            "document.title = 'dray live'; \
             window.__hit = false; \
             document.body.innerHTML = '<button id=b>Go</button>'; \
             document.getElementById('b').onclick = () => { window.__hit = true }; \
             1",
        )
        .await;

        let (title, _) = run(&session, BrowserAction::Get { what: Get::Title, at: None })
            .await
            .expect("get title failed");
        assert_eq!(title, "dray live", "the tab's own state is what `get title` reads");

        let (_, _) = run(
            &session,
            BrowserAction::Click {
                at: Locator::Role { role: "button".into(), name: Some("Go".into()), exact: false },
            },
        )
        .await
        .expect("click failed");
        assert_eq!(eval_ok(&session, "window.__hit").await, Value::Bool(true), "the click reached the page");

        eval_ok(&session, "console.log('hello from the page'); 1").await;
        let (logged, _) = run(&session, BrowserAction::Console).await.expect("console failed");
        assert!(logged.contains("hello from the page"), "console drained: {logged}");

        let (tabs, _) = run(&session, BrowserAction::Tabs).await.expect("tabs failed");
        println!("tabs: {tabs}");
        assert!(tabs.contains("about:blank"), "the open tab lists itself");

        crate::browser::close(&session).await;
    }
}
