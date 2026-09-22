//! Finding a Chromium to drive, and starting one per session.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};

/// The browsers this can drive, in the order tried.
///
/// Every one of them is Chromium underneath and speaks the same DevTools
/// protocol, so the list is about what is *installed* rather than about
/// behaviour. Plain `chromium` first: it is the one whose release cadence
/// matches the protocol this app writes against, and a reader who has it
/// almost certainly installed it on purpose.
const CANDIDATES: &[&str] = &[
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
    "brave",
    "brave-browser",
    "microsoft-edge",
    "vivaldi",
];

/// Where a browser lives besides the reader's `PATH`.
///
/// A Dock- or launcher-started bundle inherits a bare environment, the trap
/// `binpath.rs` documents for the agent CLIs — so `PATH` alone cannot be the
/// only place looked.
const DIRS: &[&str] = &["/usr/bin", "/usr/local/bin", "/opt/google/chrome", "/snap/bin"];

/// The browser to drive, or `None` where none is installed.
pub fn resolve() -> Option<PathBuf> {
    for name in CANDIDATES {
        if let Ok(path) = which(name) {
            return Some(path);
        }
        for dir in DIRS {
            let path = Path::new(dir).join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// `PATH` lookup without a crate for it: one `split` over the variable the
/// process already carries.
fn which(name: &str) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").ok_or_else(|| anyhow!("no PATH"))?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| anyhow!("{name} is not on PATH"))
}

/// A running browser and the endpoint its DevTools answers on.
pub struct Launched {
    pub child: tokio::process::Child,
    /// `http://127.0.0.1:<port>` — the base every DevTools URL hangs off.
    pub endpoint: String,
}

/// Starts a browser against `profile`, and answers once its DevTools port is up.
///
/// **Headless.** The page is drawn inside Dray from a screencast rather than in
/// a window of its own, so a second window on screen would be the same page
/// twice — and on a machine the reader is sharing, one they did not ask for.
///
/// **Port 0, and the port is read from the profile rather than from stderr.**
/// Chromium writes `DevToolsActivePort` into the user data directory as its
/// last startup step, so the file existing *is* the readiness signal; parsing
/// the banner on stderr means owning that pipe for the life of the process and
/// racing whatever else it decides to print.
pub async fn start(binary: &Path, profile: &Path) -> Result<Launched> {
    std::fs::create_dir_all(profile)
        .with_context(|| format!("could not make the browser profile at {}", profile.display()))?;
    // A port file left by a previous run would be read as this run's, and the
    // port in it is almost certainly nobody's now.
    let port_file = profile.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);

    let child = tokio::process::Command::new(binary)
        .arg("--headless=new")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        // Chromium refuses to start as root without this, and a container or a
        // misconfigured desktop is exactly where that bites with no clue why.
        .arg("--no-sandbox")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-background-timer-throttling")
        // Or a page the reader is not looking at stops rendering, which is the
        // one thing a screencast cannot survive.
        .arg("--disable-backgrounding-occluded-windows")
        .arg("--disable-renderer-backgrounding")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("could not start {}", binary.display()))?;

    let endpoint = wait_for_port(&port_file).await?;
    Ok(Launched { child, endpoint })
}

/// How long a browser gets to publish its port before this gives up.
const READY: Duration = Duration::from_secs(20);

/// Polls for the port file, since there is no event to wait on.
///
/// The file is written whole by Chromium, but a reader can still catch it
/// between `create` and `write`, so an empty or half-written file is retried
/// rather than treated as a failure.
async fn wait_for_port(port_file: &Path) -> Result<String> {
    let deadline = tokio::time::Instant::now() + READY;
    loop {
        if let Ok(text) = std::fs::read_to_string(port_file) {
            if let Some(port) = text.lines().next().filter(|line| !line.is_empty()) {
                if port.parse::<u16>().is_ok() {
                    return Ok(format!("http://127.0.0.1:{port}"));
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "the browser did not report a DevTools port within {}s",
                READY.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
