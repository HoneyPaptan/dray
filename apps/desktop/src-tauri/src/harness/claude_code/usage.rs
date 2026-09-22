//! What the account has spent against its plan, read out of the CLI's own
//! `/usage` command.
//!
//! Nothing on the stream-json wire answers this. `rate_limit_event` rides most
//! turns and is the only other source, but it carries `utilization` alone and
//! only alongside `allowed_warning` — so it says nothing at all until the
//! window is nearly full, which is the moment the reader has already felt.
//! `/usage` answers every window, always, and the reply is **synthetic**:
//! measured against v2.1.269 it comes back as an assistant message with
//! `model: "<synthetic>"` and `total_cost_usd: 0`, so this costs a process and
//! no model call.
//!
//! The windows are the CLI's own sentences, kept verbatim rather than
//! re-derived: it names them (`Current week (all models)`) and formats each
//! reset in the reader's own timezone, and a second spelling here would be a
//! second answer to a question the CLI has already answered.
//!
//! Cost, stated: the probe is an ordinary prompt, so the CLI files a session
//! for it under `~/.claude/projects` like any other. One per cache miss.

use crate::harness::ProbeCache;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{process::Stdio, sync::LazyLock, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    time::timeout,
};
use ts_rs::TS;

/// One plan window, exactly as the CLI named it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "events.ts")]
#[serde(rename_all = "camelCase")]
pub struct PlanWindow {
    /// `Current session`, `Current week (all models)`, `Current week (Fable)`.
    /// The CLI's word, drawn as written — which is what lets a window this
    /// build has never heard of still draw correctly.
    pub label: String,
    /// Whole percent spent, `33` at 33%.
    pub used_percent: u32,
    /// `Sep 28, 4:30am (Asia/Kolkata)` — already formatted, already in the
    /// reader's timezone, so nothing here parses or reformats it.
    pub resets: Option<String>,
}

/// Fresh for a minute. Long enough that opening the picker twice costs one
/// probe, short enough that the figure is still about now — and the numbers
/// move by single percent over a turn, so a minute cannot make the answer
/// wrong in a way the reader would act on.
static CACHE: LazyLock<ProbeCache<Vec<PlanWindow>>> =
    LazyLock::new(|| ProbeCache::new(Duration::from_secs(60)));

/// Generous against the ~3s measured: the reply is synthetic, but the CLI still
/// loads its config, its plugins and every `SessionStart` hook the reader has
/// before it answers.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The plan windows for the account `cwd` runs under, cached for a minute.
///
/// Keyed by directory rather than globally, since `CLAUDE_CONFIG_DIR` and a
/// project-local login make "which account is this" a question only the
/// directory can answer.
pub async fn plan_windows(cwd: &str) -> Result<Vec<PlanWindow>> {
    CACHE
        .get_or_probe(cwd, || async {
            timeout(PROBE_TIMEOUT, probe(cwd))
                .await
                .context("timed out asking the CLI for plan usage")?
        })
        .await
}

/// Spawns a throwaway child, asks it `/usage`, and kills it.
///
/// No model, no permission mode and no session id: none of them change the
/// answer, and every flag is one more way a probe can fail where a session
/// would not — the same bargain the slash-command probe next door makes.
async fn probe(cwd: &str) -> Result<Vec<PlanWindow>> {
    let mut child = Command::new(crate::binpath::claude().await)
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("couldn't start claude to read plan usage")?;

    let mut stdin = child.stdin.take().context("failed to take stdin")?;
    let stdout = child.stdout.take().context("failed to take stdout")?;

    let line = json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "text", "text": "/usage" }] },
    });
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .await?;
    stdin.flush().await?;

    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        let Some(text) = assistant_text(&line) else {
            continue;
        };

        return Ok(parse_windows(&text));
    }

    bail!("the CLI closed without answering /usage")
}

/// The text of an assistant message, or `None` for every other line.
///
/// Read loosely rather than through [`parser::ClaudeCodeEvent`]: this stream is
/// one reply and a great deal of startup noise, so a line that does not
/// classify is something to skip rather than a parse failure to report.
///
/// The synthetic marker is deliberately **not** required. It is what the CLI
/// answers a built-in with today, and a build that stopped sending it would
/// leave this reading nothing while the sentence it wants is on screen — where
/// taking any assistant text costs, at worst, a parse that finds no windows.
///
/// [`parser::ClaudeCodeEvent`]: super::parser::ClaudeCodeEvent
fn assistant_text(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type")? != "assistant" {
        return None;
    }

    let blocks = value.get("message")?.get("content")?.as_array()?;
    let text: String = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");

    (!text.is_empty()).then_some(text)
}

/// Reads every `<label>: <n>% used · resets <when>` line out of the reply.
///
/// Written to under-match, because the rest of that reply is prose about what
/// is contributing to the limit and several of those lines carry both a colon
/// and a percentage (`Top skills: /react-flow 2%`). Three things must hold at
/// once — the label opens with `Current`, a colon separates it, and the figure
/// is followed by `% used` — and a line failing any of them is prose.
///
/// An account on an API key gets no windows at all: the CLI reports a dollar
/// cost there instead, which has no `% used` in it, so this answers empty and
/// the picker falls back to what the session itself reported.
fn parse_windows(text: &str) -> Vec<PlanWindow> {
    text.lines().filter_map(parse_window).collect()
}

fn parse_window(line: &str) -> Option<PlanWindow> {
    let line = line.trim();
    if !line.starts_with("Current ") {
        return None;
    }

    let (label, rest) = line.split_once(": ")?;
    let (percent, rest) = rest.split_once("% used")?;
    let used_percent = percent.trim().parse().ok()?;

    // The separator is a middle dot the CLI draws itself; a window with no
    // reset stated is an ordinary answer rather than a line to refuse.
    let resets = rest
        .split_once("resets ")
        .map(|(_, when)| when.trim().to_string())
        .filter(|when| !when.is_empty());

    Some(PlanWindow {
        label: label.trim().to_string(),
        used_percent,
        resets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reply as captured from v2.1.269, prose and all.
    const CAPTURED: &str = "You are currently using your subscription to power your Claude Code usage

Current session: 3% used · resets Sep 23, 8am (Asia/Kolkata)
Current week (all models): 33% used · resets Sep 28, 4:30am (Asia/Kolkata)
Current week (Fable): 0% used · resets Sep 28, 4:30am (Asia/Kolkata)

What's contributing to your limits usage?
Approximate, based on local sessions on this machine.

Last 24h · 2466 requests · 61 sessions
  89% of your usage was at >150k context
  Top skills: /caveman:caveman-commit 2%
  Top plugins: caveman 2%";

    #[test]
    fn reads_every_window_and_no_prose() {
        let windows = parse_windows(CAPTURED);

        assert_eq!(windows.len(), 3);
        assert_eq!(
            windows[0],
            PlanWindow {
                label: "Current session".into(),
                used_percent: 3,
                resets: Some("Sep 23, 8am (Asia/Kolkata)".into()),
            }
        );
        assert_eq!(windows[1].label, "Current week (all models)");
        assert_eq!(windows[1].used_percent, 33);
        assert_eq!(windows[2].used_percent, 0);
    }

    /// The contributing section is the reason the match is three tests rather
    /// than one: these lines carry a colon and a percentage between them.
    #[test]
    fn prose_carrying_a_colon_and_a_percent_is_not_a_window() {
        assert!(parse_window("  Top skills: /react-flow 2%, /caveman 1%").is_none());
        assert!(parse_window("  89% of your usage was at >150k context").is_none());
        assert!(parse_window("Last 7d · 5510 requests · 86 sessions").is_none());
    }

    /// An API-key account is billed rather than metered, so the CLI reports a
    /// cost and this must answer nothing rather than half-reading it.
    #[test]
    fn a_priced_account_reports_no_windows() {
        assert!(parse_windows("Current session cost: $1.23").is_empty());
    }

    /// A window with no reset stated still counts — the figure is the half the
    /// reader acts on.
    #[test]
    fn a_window_without_a_reset_still_reads() {
        let windows = parse_windows("Current session: 12% used");

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].resets, None);
    }
}
