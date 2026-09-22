//! What the `/` picker offers for an opencode session.
//!
//! Off the wire, which is grok's arrangement rather than fx's: opencode
//! publishes `available_commands_update` on `session/update` the moment a
//! session opens — 53 rows on the machine this was written against, its own
//! `init`, `review` and `customize-opencode` plus every skill it found,
//! including the reader's `~/.claude/skills`. fx publishes an empty list and so
//! has to have its roots walked; opencode does not, and a disk walk beside a
//! wire answer is a second answer free to disagree with the first.
//!
//! What it costs is that the list lands **after** `session/new`, so a directory
//! no opencode session has run in has no answer yet — and `session/new`
//! persists a session, so probing for one would litter the reader's own
//! history. Hence a per-directory cache the read loop fills and an `Err` until
//! it does, which the picker reads as a probe still out: the menu stays shut
//! rather than claiming opencode has no commands.
//!
//! Nothing is withheld. grok's list carries TUI screens and stance controls
//! that Dray either owns or cannot follow; opencode's carries prompts and
//! skills, every one of which is a thing to send.

use crate::harness::claude_code::commands::{first_sentence, SlashCommand};
use crate::harness::ProbeCache;
use std::sync::LazyLock;
use std::time::Duration;

use super::parser::AvailableCommand;

/// Filled by a live session rather than by a probe, so nothing expires: the
/// entry is replaced whenever a session in that directory publishes again,
/// which is every `session/new`.
static CACHE: LazyLock<ProbeCache<Vec<SlashCommand>>> =
    LazyLock::new(|| ProbeCache::new(Duration::MAX));

/// What the picker draws for `cwd`, or an error where no opencode session has
/// run there yet.
pub async fn list_commands(cwd: &str) -> anyhow::Result<Vec<SlashCommand>> {
    CACHE.peek(cwd).ok_or_else(|| {
        anyhow::anyhow!("opencode has not published its commands for this directory yet")
    })
}

/// Records what a live session published.
pub fn remember(cwd: &str, published: Vec<AvailableCommand>) {
    let rows: Vec<SlashCommand> = published
        .into_iter()
        .map(|command| SlashCommand {
            // Written for the model, drawn as one line — cut at the first full
            // stop, the same reading every other harness's picker takes.
            description: first_sentence(&command.description),
            argument_hint: command
                .input
                .and_then(|input| input.hint)
                .unwrap_or_default(),
            aliases: Vec::new(),
            name: command.name,
        })
        .collect();

    // An empty publish is opencode saying nothing rather than saying "none",
    // and replacing a good list with an empty one would draw a picker claiming
    // this agent has no commands at all.
    if rows.is_empty() {
        return;
    }

    CACHE.insert(cwd, rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// opencode sends `name` and `description` and no input hint at all, so the
    /// hint has to survive being absent rather than being defaulted upstream.
    fn published() -> Vec<AvailableCommand> {
        ["init", "review", "brain-sync"]
            .iter()
            .map(|name| {
                serde_json::from_value(json!({
                    "name": name,
                    "description": "Does a thing. And then several more things.",
                }))
                .unwrap()
            })
            .collect()
    }

    #[tokio::test]
    async fn the_published_list_is_the_list() {
        let dir = "/tmp/opencode-commands-test";
        remember(dir, published());

        let rows = list_commands(dir).await.unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["init", "review", "brain-sync"]);
        assert_eq!(rows[0].description, "Does a thing.");
        assert_eq!(rows[0].argument_hint, "");
    }

    /// A directory with no session yet answers an error, never an empty list —
    /// the picker reads the second as a claim about opencode.
    #[tokio::test]
    async fn a_directory_with_no_session_yet_answers_an_error() {
        assert!(list_commands("/tmp/opencode-never-run").await.is_err());
    }

    /// An empty publish must not replace a good answer.
    #[tokio::test]
    async fn an_empty_publish_changes_nothing() {
        let dir = "/tmp/opencode-commands-empty";
        remember(dir, published());
        remember(dir, Vec::new());

        assert_eq!(list_commands(dir).await.unwrap().len(), 3);
    }
}
