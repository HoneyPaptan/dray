//! What a Cline session has cost, read off Cline's own session file.
//!
//! Nothing on the ACP wire answers it — no `usage_update`, no token count on
//! the prompt response, nothing under `_meta`, measured against 3.0.64 and
//! pinned by the parser's own test. Cline does record it, on disk: every
//! assistant message in its session file carries a `metrics` object holding
//! that request's `cost` and token counts, so the session's spend is the sum
//! down that file.
//!
//! Read at the turn's end rather than tracked as it arrives, the context ring's
//! own bargain: a reopened session reads what it read live, and nothing here
//! has to stay in step with a file a second process owns.
//!
//! The file is addressable because Cline's own session id **is** Dray's resume
//! handle — `_meta.sessionId` on `session/new` is ignored and Cline mints its
//! own, which the index records as `thread_id`.
//!
//! **Cline flushes before the turn closes, measured rather than assumed**: one
//! captured session wrote its metrics at `15:04:40.362Z` against a
//! `turn_completed` at `15:04:40.393Z`, 31ms later. So the read is exact from
//! the first turn, and a bounded wait would buy nothing — it was the
//! alternative and is refused on its own terms too, since it delays
//! `turn_completed` and the session then draws as working after it stopped.
//! Should that ordering ever move, the figure is cumulative and the next
//! turn's read carries what this one missed, so the failure is one stale
//! reading rather than a lost one.

use crate::events::Usage;
use serde::Deserialize;
use std::path::PathBuf;

/// One request's metrics, as Cline writes them. Every field but the cost is
/// read past — tokens are recorded here and drawn nowhere, and a figure this
/// app does not draw is one it should not claim to know.
#[derive(Deserialize)]
struct Metrics {
    cost: Option<f64>,
}

#[derive(Deserialize)]
struct Message {
    metrics: Option<Metrics>,
}

#[derive(Deserialize)]
struct SessionFile {
    #[serde(default)]
    messages: Vec<Message>,
}

/// What this session has spent so far, for the finished turn to carry.
///
/// `None` where the file is missing or unreadable, which is the ordinary state
/// of a session whose first turn Cline has not written yet — and deliberately
/// not the same answer as `Some(0.0)`, which is what a free model genuinely
/// costs and is the figure a reader on one wants to see.
pub fn spent(thread_id: &str) -> Option<Usage> {
    let total = session_cost(thread_id)?;
    Some(Usage {
        cost_usd: Some(total),
        ..Usage::default()
    })
}

fn session_cost(thread_id: &str) -> Option<f64> {
    let raw = std::fs::read_to_string(session_file(thread_id)?).ok()?;
    let file: SessionFile = serde_json::from_str(&raw).ok()?;
    Some(
        file.messages
            .iter()
            .filter_map(|m| m.metrics.as_ref()?.cost)
            .sum(),
    )
}

/// Cline's default data directory, which is where Dray's children write: the
/// spawn passes neither `--config` nor `--data-dir`, so overriding either is
/// not a state this can be asked about.
fn session_file(thread_id: &str) -> Option<PathBuf> {
    // A directory name, so anything that could climb out of it is refused
    // rather than joined — the id arrives from a child process.
    if thread_id.is_empty() || thread_id.contains(['/', '\\']) || thread_id.contains("..") {
        return None;
    }
    Some(
        std::env::home_dir()?
            .join(".cline/data/sessions")
            .join(thread_id)
            .join(format!("{thread_id}.messages.json")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost_of(raw: &str) -> Option<f64> {
        let file: SessionFile = serde_json::from_str(raw).ok()?;
        Some(
            file.messages
                .iter()
                .filter_map(|m| m.metrics.as_ref()?.cost)
                .sum(),
        )
    }

    /// The shape as captured from a real session: metrics ride the assistant
    /// messages alone, and the reader's own turns carry none.
    #[test]
    fn the_total_is_every_requests_cost() {
        let total = cost_of(
            r#"{"messages":[
                {"role":"user","content":[]},
                {"role":"assistant","metrics":{"inputTokens":91909,"outputTokens":116,"cost":0.00287327}},
                {"role":"user","content":[]},
                {"role":"assistant","metrics":{"inputTokens":92662,"outputTokens":146,"cost":0.000903972}}
            ]}"#,
        );

        assert_eq!(total, Some(0.00287327 + 0.000903972));
    }

    /// A free model reports a real zero, which must reach the picker as a
    /// figure rather than as "no answer" — that is the whole question a reader
    /// on a `:free` model is asking.
    #[test]
    fn a_free_model_costs_zero_rather_than_nothing() {
        let total = cost_of(r#"{"messages":[{"role":"assistant","metrics":{"cost":0}}]}"#);

        assert_eq!(total, Some(0.0));
    }

    /// A session file written before its first request answers zero, and a
    /// message shape this build has never seen costs its own cost and no more.
    #[test]
    fn an_unfamiliar_message_costs_one_figure_not_the_file() {
        assert_eq!(cost_of(r#"{"messages":[]}"#), Some(0.0));
        assert_eq!(cost_of(r#"{"version":1}"#), Some(0.0));
        assert_eq!(
            cost_of(r#"{"messages":[{"role":"assistant","metrics":{}},{"role":"assistant","metrics":{"cost":0.5}}]}"#),
            Some(0.5)
        );
    }

    /// The id names a directory and arrives from a child process, so a
    /// traversal is refused rather than joined.
    #[test]
    fn an_id_that_could_climb_out_names_no_file() {
        assert!(session_file("").is_none());
        assert!(session_file("../../etc").is_none());
        assert!(session_file("a/b").is_none());
        assert!(session_file("1790173863938_1mxZL_cli").is_some());
    }
}
