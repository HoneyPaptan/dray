//! Which apps on this machine can open a path — a session's working directory
//! from the right panel's split button, or one file from a transcript row.
//!
//! Two machines answer this, and the split is by *machine* rather than by
//! compiler: `open(1)`, `.app` bundles and `.icns` are one platform's, and
//! `.desktop` entries, `gio` and `xdg-open` are the other's. Both halves
//! compile everywhere and the call sites pick with `cfg!`, so neither can rot
//! behind a `#[cfg]` nobody builds. Anything else answers an empty list.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex},
};

use serde::Serialize;
use ts_rs::TS;

use crate::harness::Harness;

/// Which run of the menu an app belongs to. Not cosmetic: "open in Cursor" and
/// "open in Ghostty" are different asks, and a flat list of both reads as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "events.ts")]
#[serde(rename_all = "snake_case")]
pub enum ExternalAppKind {
    Editor,
    Terminal,
    /// Finder. Its own kind rather than an editor, because it is the entry
    /// every machine has and the one the reader falls back to.
    Files,
}

/// An installed app a path can be handed to.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "events.ts")]
#[serde(rename_all = "camelCase")]
pub struct ExternalApp {
    /// The bundle's absolute path, and the app's address everywhere else here.
    ///
    /// Deliberately not a bundle identifier. `open -b` hands the id to Launch
    /// Services, which picks whichever copy it has indexed — so a second
    /// install of an editor, or a stale index, silently opens the wrong one.
    /// The scan found this bundle by path; the path is what it knows.
    pub path: String,
    pub name: String,
    pub kind: ExternalAppKind,
    /// The app's own icon as a `data:` PNG, or `None` where none could be read.
    /// Absent is ordinary — a bundle can keep its icon in an asset catalog with
    /// no `.icns` beside it — so the button falls back to a glyph rather than
    /// treating this as a failure.
    pub icon: Option<String>,
}

/// One row of the curated table: the bundle's file name, what to call it, and
/// which run it sits in.
struct Known {
    /// Matched on the bundle's file name **exactly**, never as a substring.
    /// `Cloudflare WARP.app` is not `Warp.app`, and a contains-check lists it
    /// as a terminal that then ignores the directory.
    bundle: &'static str,
    name: &'static str,
    kind: ExternalAppKind,
}

const fn editor(bundle: &'static str, name: &'static str) -> Known {
    Known { bundle, name, kind: ExternalAppKind::Editor }
}

const fn terminal(bundle: &'static str, name: &'static str) -> Known {
    Known { bundle, name, kind: ExternalAppKind::Terminal }
}

/// The apps this menu knows how to hand a path to, in the order drawn.
///
/// Curated rather than discovered, and the curation *is* the guarantee: every
/// entry here takes a directory as `open -a <bundle> <dir>`, and every
/// [`ExternalAppKind::Editor`] takes a single file the same way — which is what
/// a filename in the transcript is handed to. Reading each found
/// bundle's `CFBundleDocumentTypes` for `public.folder` was the alternative and
/// it is the wrong gate — `open -a` names the app outright rather than asking
/// Launch Services to rank handlers, so it reaches apps that declare nothing
/// (MacVim declares no folder type and opens one fine). Gating on the
/// declaration would hide apps that work.
///
/// An app absent from this table costs one entry, never the list.
const KNOWN: &[Known] = &[
    editor("Visual Studio Code.app", "VS Code"),
    editor("Visual Studio Code - Insiders.app", "VS Code Insiders"),
    editor("Cursor.app", "Cursor"),
    editor("Windsurf.app", "Windsurf"),
    editor("Zed.app", "Zed"),
    editor("Zed Preview.app", "Zed Preview"),
    editor("Sublime Text.app", "Sublime Text"),
    editor("Nova.app", "Nova"),
    editor("BBEdit.app", "BBEdit"),
    editor("Xcode.app", "Xcode"),
    editor("IntelliJ IDEA.app", "IntelliJ IDEA"),
    editor("IntelliJ IDEA CE.app", "IntelliJ IDEA CE"),
    editor("WebStorm.app", "WebStorm"),
    editor("PyCharm.app", "PyCharm"),
    editor("PyCharm CE.app", "PyCharm CE"),
    editor("GoLand.app", "GoLand"),
    editor("RustRover.app", "RustRover"),
    editor("CLion.app", "CLion"),
    editor("PhpStorm.app", "PhpStorm"),
    editor("RubyMine.app", "RubyMine"),
    editor("DataGrip.app", "DataGrip"),
    editor("Android Studio.app", "Android Studio"),
    editor("VimR.app", "VimR"),
    editor("MacVim.app", "MacVim"),
    terminal("Ghostty.app", "Ghostty"),
    terminal("iTerm.app", "iTerm"),
    terminal("Warp.app", "Warp"),
    terminal("WezTerm.app", "WezTerm"),
    terminal("Terminal.app", "Terminal"),
];

/// Finder, which is in none of the scanned directories and on every mac.
const FINDER: &str = "/System/Library/CoreServices/Finder.app";

/// Where a `.app` bundle lives. One `read_dir` each and no `mdfind`: Spotlight
/// can be off or still indexing, and the common case is an app that is not
/// installed at all — where a per-app query is a spawn that answers nothing.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        // Setapp keeps its own subfolder under /Applications.
        PathBuf::from("/Applications/Setapp"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    // Where JetBrains Toolbox and per-user installs land.
    if let Some(home) = std::env::home_dir() {
        dirs.push(home.join("Applications"));
    }
    dirs
}

/// Bundle file name to its path, for every `.app` in `dirs`.
fn installed(dirs: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".app") {
                continue;
            }
            // First directory wins, so a machine-wide install outranks the
            // user's own copy — the order Launch Services itself prefers.
            found.entry(name).or_insert_with(|| entry.path());
        }
    }
    found
}

/// Icons cost two spawns each, so they are read once per bundle path and held
/// for the life of the process. Keyed by path, so an app that moves is read
/// again rather than drawn with the old one.
static ICONS: LazyLock<Mutex<HashMap<PathBuf, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The `.icns` a bundle names as its icon.
///
/// `CFBundleIconFile` may or may not carry the extension, so both spellings are
/// tried; `AppIcon` is the fallback for a bundle that names none.
fn icns_path(bundle: &Path) -> Option<PathBuf> {
    let resources = bundle.join("Contents/Resources");

    let named = Command::new("plutil")
        .args(["-extract", "CFBundleIconFile", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|name| !name.is_empty());

    named
        .into_iter()
        .chain(std::iter::once("AppIcon".to_string()))
        .flat_map(|name| {
            let with_ext = if name.ends_with(".icns") {
                name.clone()
            } else {
                format!("{name}.icns")
            };
            [resources.join(with_ext), resources.join(name)]
        })
        .find(|path| path.is_file())
}

/// The bundle's icon as a `data:` PNG, read through `sips` — on every mac, and
/// the only thing here that turns an `.icns` into something a webview draws.
fn read_icon(bundle: &Path) -> Option<String> {
    if let Ok(cache) = ICONS.lock() {
        if let Some(hit) = cache.get(bundle) {
            return hit.clone();
        }
    }

    let icon = icns_path(bundle).and_then(|icns| {
        // A temp file rather than stdout: `sips` writes the paths it worked on
        // to stdout and only ever writes the image to a path.
        let out = std::env::temp_dir().join(format!("dray-appicon-{}.png", uuid::Uuid::now_v7()));
        let ok = Command::new("sips")
            .args(["-s", "format", "png", "-Z", "64"])
            .arg(&icns)
            .arg("--out")
            .arg(&out)
            .output()
            .is_ok_and(|done| done.status.success());

        let bytes = ok.then(|| std::fs::read(&out).ok()).flatten();
        let _ = std::fs::remove_file(&out);

        bytes.map(|bytes| {
            use base64::Engine as _;
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        })
    });

    if let Ok(mut cache) = ICONS.lock() {
        cache.insert(bundle.to_path_buf(), icon.clone());
    }
    icon
}

/// Every known app in `installed`, in [`KNOWN`]'s order, then Finder — with no
/// icons yet. Split from [`detect`] so the order can be pinned by a test that
/// spawns nothing.
///
/// Finder sits at the end rather than the front on purpose: it is the entry
/// every machine has, so leading with it puts the least specific answer where
/// the eye lands first.
fn resolve(installed: &HashMap<String, PathBuf>, finder: &Path) -> Vec<ExternalApp> {
    let mut apps: Vec<ExternalApp> = KNOWN
        .iter()
        .filter_map(|known| {
            let path = installed.get(known.bundle)?;
            Some(ExternalApp {
                path: path.to_string_lossy().into_owned(),
                name: known.name.to_string(),
                kind: known.kind,
                icon: None,
            })
        })
        .collect();

    if finder.is_dir() {
        apps.push(ExternalApp {
            path: finder.to_string_lossy().into_owned(),
            name: "Finder".to_string(),
            kind: ExternalAppKind::Files,
            icon: None,
        });
    }

    apps
}

fn detect() -> Vec<ExternalApp> {
    let mut apps = resolve(&installed(&search_dirs()), Path::new(FINDER));
    for app in &mut apps {
        app.icon = read_icon(Path::new(&app.path));
    }
    apps
}

/// The apps that can open a path on this machine.
///
/// Re-scanned on every call rather than held for the process: the walk is a
/// handful of `read_dir`s, and only the icons are dear enough to cache — so an
/// editor installed while Dray is running shows up the next time the panel
/// asks, instead of needing a restart the way the slash-command cache does.
#[tauri::command]
pub async fn list_open_apps() -> Vec<ExternalApp> {
    if cfg!(target_os = "macos") {
        return tauri::async_runtime::spawn_blocking(detect).await.unwrap_or_default();
    }
    if cfg!(target_os = "linux") {
        return tauri::async_runtime::spawn_blocking(xdg::detect).await.unwrap_or_default();
    }
    Vec::new()
}

/// Hands `path` to the app at `app_path`.
///
/// `open -a <bundle> <path>` and not `open -b <id>`, for [`ExternalApp::path`]'s
/// reason. A directory or a file alike: `open -a` names the app outright, and
/// Launch Services hands it whichever the path is. Nothing here reaches a
/// shell, so injection is not the risk — but a path whose name opens with `-`
/// parses as a flag, which `--` ends.
#[tauri::command]
pub async fn open_in_app(app_path: String, path: String) -> Result<(), String> {
    if cfg!(target_os = "linux") {
        return xdg::open_in_app(&app_path, &path).await;
    }

    let out = tokio::process::Command::new("open")
        .arg("-a")
        .arg(&app_path)
        .arg("--")
        .arg(&path)
        .output()
        .await
        .map_err(|err| format!("could not run open: {err}"))?;

    if out.status.success() {
        return Ok(());
    }

    // `open`'s own sentence names the cure — a moved bundle, a directory that
    // is gone — where the exit code names nothing.
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("could not open {path}")
    } else {
        stderr
    })
}

/// Terminal.app, and never the terminal the reader picked in the panel beside
/// this.
///
/// Measured, because `open` reports nothing either way: handed a `.command`
/// file, Terminal.app runs it, while Ghostty and Warp accept the open, exit 0
/// and run nothing at all. Ghostty wants `open -na <bundle> --args -e <cmd>`
/// and every other terminal its own argv, so honouring the pick means a second
/// table — and the [`KNOWN`] one above promises only `open -a <bundle> <dir>`,
/// which is a different shape from this. A button that silently does nothing
/// on two of the five terminals that table lists is worse than one that always
/// works, the same reading `pick_file_opener` takes with Finder.
///
/// Every mac has it, so there is no case where this resolves to nothing.
const TERMINAL: &str = "/System/Applications/Utilities/Terminal.app";

/// Opens a terminal at `cwd` running the harness's login command.
///
/// No shell string crosses the bridge: the caller names a [`Harness`] and the
/// command is composed here, from the resolved binary and
/// [`Harness::login_args`]. Same reading `permissions.rs` takes — the rule
/// never leaves Rust, so nothing the frontend holds can widen it.
///
/// The mechanism is a throwaway `.command` script rather than
/// `osascript -e 'tell application "Terminal" to do script …'`, which needs
/// macOS Automation permission: that prompts, can be denied, and a denial is
/// silent. A `.command` file needs no permission at all.
#[tauri::command]
pub async fn open_login_terminal(harness: Harness, cwd: String) -> Result<(), String> {
    let binary = crate::binpath::agent_binary(harness).await;
    let mut command = sh_quote(&binary.to_string_lossy());
    for arg in harness.login_args() {
        command.push(' ');
        command.push_str(arg);
    }

    run_in_terminal(&command, &cwd).await
}

/// Runs one shell line in Terminal.app at `cwd`.
///
/// **The caller owns what is in that line**, and every one of them composes it
/// from a closed set — this harness's [`Harness::login_args`], or an
/// [`crate::accounts::AuthOption`] whose command is a literal in that table.
/// Nothing a frontend typed may reach here: the string is written into a shell
/// script, so the safety is in where it came from and not in any escaping this
/// could do to it.
pub(crate) async fn run_in_terminal(command: &str, cwd: &str) -> Result<(), String> {
    // Terminal runs a `.command` from the reader's home, not from the script's
    // own directory, so the `cd` is what puts the login in the session's tree.
    // Omitted where there is no directory to name — the Accounts tab is reached
    // with no session selected — since `cd ''` fails and would take the command
    // with it. Self-delete last: a reader who closes the window mid-login leaks
    // one file into a temp dir macOS reaps on its own.
    let enter = if cwd.is_empty() {
        String::new()
    } else {
        format!("cd {} || exit 1\n", sh_quote(cwd))
    };
    let script = format!("#!/bin/sh\n{enter}{command}\nrm -f -- \"$0\"\n");

    // `.command` is the extension Terminal.app *runs* a file by rather than
    // opening it in an editor. Nothing on Linux reads it, where `.sh` is what
    // somebody finding one of these in a temp dir would expect.
    let suffix = if cfg!(target_os = "macos") { "command" } else { "sh" };
    let path = std::env::temp_dir().join(format!("dray-login-{}.{suffix}", uuid::Uuid::now_v7()));
    write_script(&path, &script)
        .map_err(|err| format!("could not write the login script: {err}"))?;

    if cfg!(target_os = "linux") {
        // Cleaned up only where nothing was started. A terminal that did open
        // runs the script, which deletes itself on the way out.
        return xdg::run_in_terminal(&path).await.inspect_err(|_| {
            let _ = std::fs::remove_file(&path);
        });
    }

    let out = tokio::process::Command::new("open")
        .arg("-a")
        .arg(TERMINAL)
        .arg("--")
        .arg(&path)
        .output()
        .await
        .map_err(|err| format!("could not run open: {err}"))?;

    if out.status.success() {
        return Ok(());
    }

    let _ = std::fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        "could not open Terminal".to_string()
    } else {
        stderr
    })
}

/// Writes the script at `0700` on the *create*, never by a `chmod` after.
///
/// Same reading the tracker key's own write records (`issues.rs`): `fs::write`
/// creates at the process umask, so any other order leaves a window where the
/// file is world-readable — and this one is also world-*executable*, which is a
/// file Terminal will run. `create_new` beside it because a leftover from a
/// crashed write could be somebody else's at whatever mode they chose. `0700`
/// is what makes it executable at all, which `.command` needs.
fn write_script(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(path)?
        }
        #[cfg(not(unix))]
        {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
        }
    };

    file.write_all(body.as_bytes())
}

/// Quotes one word for `/bin/sh`.
///
/// The cwd is a real path from the index and can hold a space, a quote or a
/// dollar sign, and it is being written into a file that gets executed — so
/// single quotes, with the only character they cannot carry spliced in from
/// outside them.
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The XDG half: `.desktop` entries, `gio launch`, and the terminals Linux has
/// instead of `open -a`.
///
/// Compiled on every platform and reached through `cfg!` at the call sites, the
/// same bargain the macOS half takes. Nothing in here needs the compiler to know
/// which machine it is on — a mac simply has none of these directories and this
/// answers an empty list there.
mod xdg {
    use std::{collections::HashMap, path::PathBuf};

    use super::{ExternalApp, ExternalAppKind};

    /// One row: the desktop entry's id — its file name without `.desktop` —
    /// what to call it, and which run of the menu it belongs to.
    struct Known {
        id: &'static str,
        name: &'static str,
        kind: ExternalAppKind,
    }

    const fn editor(id: &'static str, name: &'static str) -> Known {
        Known { id, name, kind: ExternalAppKind::Editor }
    }

    const fn terminal(id: &'static str, name: &'static str) -> Known {
        Known { id, name, kind: ExternalAppKind::Terminal }
    }

    /// The entries this menu knows how to hand a path to, in the order drawn.
    ///
    /// Curated for the macOS table's reason, and with one difference that is
    /// this platform's own: an app is packaged by several hands here, so one app
    /// can answer to several ids — `Alacritty` from the tarball and `alacritty`
    /// from the distribution, `dev.zed.Zed` and `zed`. Both spellings are listed
    /// and [`detect`] keeps the first, since a machine carrying both would
    /// otherwise draw the same app twice.
    const KNOWN: &[Known] = &[
        editor("code", "VS Code"),
        editor("visual-studio-code", "VS Code"),
        editor("code-insiders", "VS Code Insiders"),
        editor("codium", "VSCodium"),
        editor("cursor", "Cursor"),
        editor("windsurf", "Windsurf"),
        editor("dev.zed.Zed", "Zed"),
        editor("zed", "Zed"),
        editor("sublime_text", "Sublime Text"),
        editor("jetbrains-idea", "IntelliJ IDEA"),
        editor("jetbrains-idea-ce", "IntelliJ IDEA CE"),
        editor("jetbrains-webstorm", "WebStorm"),
        editor("jetbrains-pycharm", "PyCharm"),
        editor("jetbrains-pycharm-ce", "PyCharm CE"),
        editor("jetbrains-goland", "GoLand"),
        editor("jetbrains-rustrover", "RustRover"),
        editor("jetbrains-clion", "CLion"),
        editor("jetbrains-phpstorm", "PhpStorm"),
        editor("jetbrains-rubymine", "RubyMine"),
        editor("jetbrains-datagrip", "DataGrip"),
        editor("android-studio", "Android Studio"),
        editor("nvim", "Neovim"),
        editor("gvim", "Vim"),
        editor("org.gnome.TextEditor", "Text Editor"),
        terminal("com.mitchellh.ghostty", "Ghostty"),
        terminal("ghostty", "Ghostty"),
        terminal("kitty", "kitty"),
        terminal("Alacritty", "Alacritty"),
        terminal("alacritty", "Alacritty"),
        terminal("org.wezfurlong.wezterm", "WezTerm"),
        terminal("org.gnome.Console", "Console"),
        terminal("org.gnome.Terminal", "Terminal"),
        terminal("konsole", "Konsole"),
        terminal("foot", "foot"),
    ];

    /// Finder's counterpart, and it is a *command* rather than an entry.
    ///
    /// Which app manages files is the reader's own association and `xdg-open`
    /// already resolves it, so there is nothing to scan for and nothing to get
    /// wrong when they change it. It doubles as this half's sentinel: it is not
    /// a path, so [`open_in_app`] tells it from every other row by value.
    pub const FILES: &str = "xdg-open";

    /// Where a `.desktop` entry lives, in precedence order.
    ///
    /// The reader's own first, then flatpak's per-user exports, then whatever
    /// `XDG_DATA_DIRS` names, then the system-wide flatpak and snap exports that
    /// most distributions put on that variable and not all of them do.
    fn desktop_dirs() -> Vec<PathBuf> {
        let home = std::env::home_dir();
        let mut dirs = Vec::new();

        match std::env::var_os("XDG_DATA_HOME").filter(|home| !home.is_empty()) {
            Some(data_home) => dirs.push(PathBuf::from(data_home).join("applications")),
            None => {
                if let Some(home) = &home {
                    dirs.push(home.join(".local/share/applications"));
                }
            }
        }
        if let Some(home) = &home {
            dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
        }

        let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        let data_dirs = if data_dirs.is_empty() {
            "/usr/local/share:/usr/share".to_string()
        } else {
            data_dirs
        };
        for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
            dirs.push(PathBuf::from(dir).join("applications"));
        }

        dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
        dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));
        dirs
    }

    /// Entry id to its path, for every `.desktop` file in `dirs`.
    ///
    /// `or_insert`, because the list above is in precedence order and an entry
    /// the reader installed for themselves has to outrank the system's copy of
    /// the same id — the macOS half's "first directory wins", said again.
    fn installed(dirs: &[PathBuf]) -> HashMap<String, PathBuf> {
        let mut found = HashMap::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext != "desktop") {
                    continue;
                }
                if let Some(id) = path.file_stem().and_then(|id| id.to_str()) {
                    found.entry(id.to_string()).or_insert(path);
                }
            }
        }
        found
    }

    /// The apps that can open a path on this machine.
    pub fn detect() -> Vec<ExternalApp> {
        let installed = installed(&desktop_dirs());

        let mut apps: Vec<ExternalApp> = Vec::new();
        for known in KNOWN {
            let Some(path) = installed.get(known.id) else {
                continue;
            };
            if apps.iter().any(|app| app.name == known.name) {
                continue;
            }
            apps.push(ExternalApp {
                path: path.to_string_lossy().into_owned(),
                name: known.name.to_string(),
                kind: known.kind,
                // An icon here is a name to look up in the reader's theme rather
                // than a file beside the app, so reading one means walking that
                // theme's inheritance and its sizes. The button already falls
                // back to a glyph, and absent is the ordinary case on macOS too.
                icon: None,
            });
        }

        // Unconditional, where the macOS half checks Finder is really there:
        // this row is a command every desktop has rather than an app that might
        // not be installed.
        apps.push(ExternalApp {
            path: FILES.to_string(),
            name: "File manager".to_string(),
            kind: ExternalAppKind::Files,
            icon: None,
        });
        apps
    }

    /// Hands `path` to the entry at `app_path`, or to the reader's own default.
    pub async fn open_in_app(app_path: &str, path: &str) -> Result<(), String> {
        let mut command = if app_path == FILES {
            // Finder *reveals* a file. `xdg-open` has no such verb, and handed
            // one it would open it in whatever app claims the type — which is
            // the editor rows' job, not this one's. The containing directory is
            // the closest honest answer, and it is what the button promises.
            let target = std::path::Path::new(path);
            let target = if target.is_file() { target.parent().unwrap_or(target) } else { target };
            let mut command = tokio::process::Command::new("xdg-open");
            command.arg(target);
            command
        } else {
            // `gio launch` runs the entry's own `Exec` line with the path
            // appended, which is what makes an editor's `%U` handling apply —
            // including the `vscode://file/…:12` a transcript link carries a
            // line number in. Naming the entry outright is the macOS half's rule
            // as well: nothing here asks the desktop to rank a handler.
            let mut command = tokio::process::Command::new("gio");
            command.arg("launch").arg(app_path).arg(path);
            command
        };

        let out = command
            .output()
            .await
            .map_err(|err| format!("could not run the opener: {err}"))?;

        if out.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("could not open {path}")
        } else {
            stderr
        })
    }

    /// The terminals this can start, in the order tried, each with the argument
    /// it takes a command by.
    ///
    /// A list rather than the terminal the reader picked in the panel, for the
    /// macOS half's reason: that pick promises only that the app opens a
    /// *directory*, and running a command in one is a different shape with a
    /// different flag per terminal. There is no `x-terminal-emulator` on most of
    /// these distributions, so the alternative is no terminal at all.
    const TERMINALS: &[(&str, &[&str])] = &[
        ("ghostty", &["-e"]),
        ("kitty", &[]),
        ("wezterm", &["start", "--"]),
        ("alacritty", &["-e"]),
        ("foot", &[]),
        ("konsole", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("xfce4-terminal", &["-x"]),
        ("xterm", &["-e"]),
    ];

    /// Opens the first terminal that is installed, running `script`.
    ///
    /// Spawned rather than waited on: the window lives as long as the reader
    /// wants it. A terminal that is simply not installed fails here at the
    /// `exec`, which is what makes walking the list cheap — no `which`, no probe
    /// — and the error kind is what tells that case from a real failure.
    pub async fn run_in_terminal(script: &std::path::Path) -> Result<(), String> {
        for (binary, args) in TERMINALS {
            let mut command = tokio::process::Command::new(binary);
            command.args(*args).arg(script);
            match command.spawn() {
                Ok(_) => return Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(format!("could not start {binary}: {err}")),
            }
        }
        Err("No terminal found. Copy the command and run it yourself.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quoted word is written into a file that gets executed, so a cwd
    /// holding a quote must stay one argument rather than becoming a command.
    #[test]
    fn sh_quote_contains_a_quote() {
        assert_eq!(sh_quote("/tmp/plain"), "'/tmp/plain'");
        assert_eq!(sh_quote("/tmp/my project"), "'/tmp/my project'");
        assert_eq!(sh_quote("/tmp/$HOME"), "'/tmp/$HOME'");
        assert_eq!(sh_quote("/tmp/it's"), r"'/tmp/it'\''s'");
        assert_eq!(
            sh_quote("/tmp/a'; rm -rf /; echo '"),
            r"'/tmp/a'\''; rm -rf /; echo '\'''"
        );
    }

    /// The table is matched on exact file names, so an extension-less entry is
    /// one that can never be found and nothing else would say so.
    #[test]
    fn every_known_bundle_names_an_app() {
        for known in KNOWN {
            assert!(
                known.bundle.ends_with(".app"),
                "{} is not a bundle name",
                known.bundle
            );
            assert!(!known.name.is_empty());
        }
    }

    /// Two entries resolving to one bundle would draw the same app twice.
    #[test]
    fn known_bundles_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for known in KNOWN {
            assert!(seen.insert(known.bundle), "{} listed twice", known.bundle);
        }
    }

    fn bundle(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// One scan over a directory of fake bundles pins the three things worth
    /// pinning: the match is exact (`Cloudflare WARP.app` is not `Warp.app`,
    /// and a substring match would list it as a terminal that ignores the
    /// directory it is handed), the order is the table's, and Finder comes
    /// last. Icons are not read, so nothing spawns.
    #[test]
    fn resolves_in_table_order_with_finder_last() {
        let tmp = std::env::temp_dir().join(format!("dray-apps-{}", uuid::Uuid::now_v7()));
        let apps_dir = bundle(&tmp, "Applications");
        // Deliberately created in the order that would be wrong if read_dir
        // order leaked through.
        bundle(&apps_dir, "Ghostty.app");
        bundle(&apps_dir, "Cloudflare WARP.app");
        bundle(&apps_dir, "Cursor.app");
        bundle(&apps_dir, "Not An App");
        let finder = bundle(&tmp, "Finder.app");

        let found = resolve(&installed(&[apps_dir]), &finder);
        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Cursor", "Ghostty", "Finder"]);
        assert_eq!(found[2].kind, ExternalAppKind::Files);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// A bundle present in two searched directories resolves to the first —
    /// the machine-wide install over the user's own copy, matching Launch
    /// Services. A `HashMap` insert in the other order would silently flip it.
    #[test]
    fn first_directory_wins() {
        let tmp = std::env::temp_dir().join(format!("dray-apps-{}", uuid::Uuid::now_v7()));
        let system = bundle(&tmp, "Applications");
        let user = bundle(&tmp, "home/Applications");
        let expected = bundle(&system, "Zed.app");
        bundle(&user, "Zed.app");

        let found = installed(&[system, user]);
        assert_eq!(found["Zed.app"], expected);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// What this machine actually resolves, printed rather than asserted:
    /// which apps are installed is a fact about the machine, so there is no
    /// count to pin. Kept because detection failing silently — an empty menu —
    /// is the one failure here that looks exactly like "nothing is installed".
    ///
    /// `cargo test -p dray --lib apps -- --ignored --nocapture`
    #[test]
    #[ignore = "reports what is installed; asserts nothing"]
    fn report_detected_apps() {
        for app in detect() {
            println!(
                "{:?}\t{}\t{}\ticon={}",
                app.kind,
                app.name,
                app.path,
                app.icon.map_or("none".into(), |icon| format!("{}b", icon.len()))
            );
        }
    }
}
