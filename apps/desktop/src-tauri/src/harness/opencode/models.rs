//! Which models opencode can run here, asked of opencode.
//!
//! pi's problem and pi's answer: the list is whatever providers the reader has
//! logged into, 388 of them on the machine this was written against, and no
//! table here could name them. So `opencode models` is the source.
//!
//! That command rather than the ACP session's own `configOptions`, which carry
//! display names this does not get. `session/new` **persists a session** — it
//! is what fills `opencode session list` — so a probe for the picker would
//! litter the reader's own history every two minutes for a prettier label.
//! `opencode models` starts no session and writes nothing.
//!
//! The id is therefore the label, which is pi's bargain too: a discovered model
//! has no name but its own, and `openrouter/anthropic/claude-opus-5` already
//! says who serves it.

use crate::harness::ProbeCache;
use crate::models::{Model, ModelId};
use anyhow::{Context, Result};
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// How long a cached answer stands. A provider logged into while Dray is open
/// is exactly the one the reader then tries to use, so this expires where the
/// slash-command cache does not — pi's reading, for pi's reason.
const FRESH_FOR: Duration = Duration::from_secs(120);

/// Generous against the ~1s measured: the binary loads the reader's whole
/// config and every plugin before it prints a line.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// One entry, keyed by nothing: the list follows the reader's logins, not a
/// directory.
static CACHE: LazyLock<ProbeCache<Vec<Model>>> = LazyLock::new(|| ProbeCache::new(FRESH_FOR));

/// Every model opencode reports, freshly read or cached.
///
/// Failure answers an empty list rather than an error, the same bargain pi's
/// makes: the picker draws its own empty state, and a reader with no provider
/// configured is in an ordinary state rather than a broken one.
pub async fn list() -> Vec<Model> {
    CACHE.get_or_probe("", probe).await.unwrap_or_else(|err| {
        eprintln!("[opencode models] {err:#}");
        Vec::new()
    })
}

/// The model with this id, from whatever opencode last reported.
///
/// `None` for the unset sentinel — opencode picking its own default — and for
/// an id no configured provider serves, which opencode refuses in its own words
/// at the spawn far better than a guess here could.
pub async fn find(id: &ModelId) -> Option<Model> {
    if id.is_unset() {
        return None;
    }

    list().await.into_iter().find(|m| &m.id == id)
}

/// Drops the cached answer, so the next read asks again. For the refresh a
/// reader asks for by hand after logging a provider in.
pub fn forget() {
    CACHE.forget();
}

async fn probe() -> Result<Vec<Model>> {
    let bin = crate::binpath::opencode().await;
    let output = timeout(
        PROBE_TIMEOUT,
        Command::new(&bin)
            .arg("models")
            .env("PATH", crate::harness::agent_path(&bin))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("timed out asking opencode for its models")?
    .context("couldn't start opencode to ask for its models")?;

    Ok(parse_list(&String::from_utf8_lossy(&output.stdout)))
}

/// One model per line, `provider/model`, and nothing else on the line.
///
/// Written to under-match: opencode prints a banner on some subcommands, and a
/// row invented out of one would be a model the spawn then refuses. A line
/// earns its place by carrying a slash, no whitespace, and a non-empty name on
/// both sides.
fn parse_list(stdout: &str) -> Vec<Model> {
    stdout.lines().filter_map(parse_model).collect()
}

fn parse_model(line: &str) -> Option<Model> {
    let id = line.trim();
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        return None;
    }

    let (provider, name) = id.split_once('/')?;
    if provider.is_empty() || name.is_empty() {
        return None;
    }

    Some(Model {
        id: ModelId::new(id),
        label: id.to_string(),
        // opencode exposes no reasoning level at all: a session's
        // `configOptions` are model and mode, measured against 1.18.30. So the
        // composer draws no effort control, and no level is ever sent.
        efforts: Vec::new(),
        default_effort: None,
        arg: id.to_string(),
        provider: provider.to_string(),
        // Its ACP handshake reports `promptCapabilities.image: true` for the
        // agent as a whole, so the tray is offered on every row. A model that
        // cannot take one refuses at the send, which is opencode's own answer
        // to give.
        accepts_images: true,
        // Every row is secondary: 388 models is not a list Shift+Tab can walk,
        // so the top level is the reader's starred shortlist and this list is
        // what the library dialog draws. pi's rule, pi's reason.
        secondary: true,
        supports_fast: false,
        free: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As captured from `opencode models` on 1.18.30.
    const CAPTURED: &str = "opencode/big-pickle
openrouter/anthropic/claude-opus-5
anthropic/claude-sonnet-5
";

    #[test]
    fn reads_every_line_as_a_model() {
        let models = parse_list(CAPTURED);

        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id.as_str(), "opencode/big-pickle");
        assert_eq!(models[0].provider, "opencode");
        // The provider is the first segment, so a three-part id keeps the rest
        // whole rather than being split twice.
        assert_eq!(models[1].provider, "openrouter");
        assert_eq!(models[1].arg, "openrouter/anthropic/claude-opus-5");
    }

    /// Anything that is not bare `provider/model` is prose, and a model
    /// invented out of prose is one the spawn refuses.
    #[test]
    fn prose_is_not_a_model() {
        assert!(parse_model("list all available models").is_none());
        assert!(parse_model("").is_none());
        assert!(parse_model("nothingness").is_none());
        assert!(parse_model("/leading").is_none());
        assert!(parse_model("trailing/").is_none());
    }

    /// No level exists to send, so no level may be offered — a rung drawn here
    /// would be one opencode has nowhere to put.
    #[test]
    fn no_model_offers_an_effort_level() {
        assert!(parse_list(CAPTURED).iter().all(|m| m.efforts.is_empty()));
    }
}
