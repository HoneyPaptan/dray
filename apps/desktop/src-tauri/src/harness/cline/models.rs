//! Which models Cline can run here, asked of Cline.
//!
//! pi's problem again — 308 of them on the machine this was written against,
//! following whichever provider the reader signed in to, and no table here
//! could name them.
//!
//! **Asked over ACP, which opencode's own module could not do.** There the
//! probe is a CLI subcommand because `session/new` persists a session and a
//! probe every two minutes would litter the reader's history. Cline does not:
//! measured, three promptless `session/new` calls left `cline history` saying
//! "No history found". So the probe opens a real session, reads the list and
//! kills the child — and that list is the only place a model's **display name**
//! exists, where `cline models` does not exist as a subcommand at all.
//!
//! The cost, stated: a probe costs a child and an authenticated round trip
//! where opencode's costs a subcommand. It is cached for the same two minutes
//! and for the same reason — a provider signed in to while Dray is open is
//! exactly the one the reader then tries to use.

use crate::harness::codex::rpc::{Incoming, RpcClient};
use crate::harness::ProbeCache;
use crate::models::{Model, ModelId};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

use super::parser::SessionOpened;

/// How long a cached answer stands.
const FRESH_FOR: Duration = Duration::from_secs(120);

/// Generous against the ~2s measured: the binary loads the reader's config and
/// asks its provider for a catalogue before it answers.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// One entry, keyed by nothing: the list follows the reader's login, not a
/// directory.
static CACHE: LazyLock<ProbeCache<Vec<Model>>> = LazyLock::new(|| ProbeCache::new(FRESH_FOR));

/// Every model Cline reports, freshly read or cached.
///
/// Failure answers an empty list rather than an error, the same bargain pi's
/// and opencode's make: the picker draws its own empty state, and a reader who
/// has not run `cline auth` yet is in an ordinary state rather than a broken
/// one.
pub async fn list() -> Vec<Model> {
    CACHE.get_or_probe("", probe).await.unwrap_or_else(|err| {
        eprintln!("[cline models] {err:#}");
        Vec::new()
    })
}

/// The model with this id, from whatever Cline last reported.
///
/// `None` for the unset sentinel — Cline picking its own default — and for an
/// id the signed-in provider does not serve, which Cline refuses in its own
/// words far better than a guess here could.
pub async fn find(id: &ModelId) -> Option<Model> {
    if id.is_unset() {
        return None;
    }

    list().await.into_iter().find(|m| &m.id == id)
}

/// Drops the cached answer, so the next read asks again. For the refresh a
/// reader asks for by hand after signing in.
pub fn forget() {
    CACHE.forget();
}

/// Spawns a throwaway `cline --acp`, opens a session, reads its model list and
/// kills the child.
async fn probe() -> Result<Vec<Model>> {
    timeout(PROBE_TIMEOUT, ask())
        .await
        .context("timed out asking cline for its models")?
}

async fn ask() -> Result<Vec<Model>> {
    let bin = crate::binpath::cline().await;
    let mut child = Command::new(&bin)
        .arg("--acp")
        .env("PATH", crate::harness::agent_path(&bin))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("couldn't start cline to ask for its models")?;

    let stdin = child.stdin.take().context("failed to take stdin")?;
    let stdout = child.stdout.take().context("failed to take stdout")?;
    let client = RpcClient::new(stdin);

    tokio::spawn({
        let client = client.clone();
        async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // Only answers matter here: the probe sends no prompt, so
                // anything else Cline says is not about us.
                if let Incoming::Malformed = client.accept(&line).await {
                    continue;
                }
            }
        }
    });

    let opened = read_list(&client).await;
    client.close();
    let _ = child.kill().await;
    opened
}

async fn read_list(client: &RpcClient) -> Result<Vec<Model>> {
    client
        .request(
            "initialize",
            json!({
                "protocolVersion": super::PROTOCOL_VERSION,
                "clientCapabilities": {},
                "clientInfo": {"name": "dray", "title": "Dray", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;

    // The reader's home rather than a project: the list follows their login,
    // and a directory would only decide which `.clinerules` the child loads.
    let cwd = std::env::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());

    let answer = client
        .request("session/new", json!({"cwd": cwd, "mcpServers": []}))
        .await?;

    Ok(models_of(&answer))
}

/// Reads the model rows off a `session/new` or `session/load` reply.
pub fn models_of(answer: &Value) -> Vec<Model> {
    let opened: SessionOpened = match serde_json::from_value(answer.clone()) {
        Ok(opened) => opened,
        Err(err) => {
            eprintln!("[cline models] could not read the session reply: {err}");
            return Vec::new();
        }
    };

    opened
        .models
        .available_models
        .into_iter()
        .filter(|row| !row.model_id.is_empty())
        .map(|row| {
            let provider = row
                .model_id
                .split_once('/')
                .map(|(p, _)| p)
                .unwrap_or("cline")
                // A leading `~` marks Cline's own hosted routing
                // (`~openai/gpt-sol-latest`) and is not part of the provider's
                // name, so the picker's heading reads `openai` either way.
                .trim_start_matches('~')
                .to_string();

            Model {
                id: ModelId::new(&row.model_id),
                // Cline's own display name where it sent one, which is the
                // whole reason this list is read over ACP rather than guessed
                // from an id.
                label: if row.name.is_empty() {
                    row.model_id.clone()
                } else {
                    row.name
                },
                // No reasoning level is reachable: a session's `configOptions`
                // are provider, model, mode and auto-approve, measured against
                // 3.0.64. The CLI's own `--thinking` flag has no ACP surface.
                efforts: Vec::new(),
                default_effort: None,
                arg: row.model_id.clone(),
                provider,
                // Its handshake reports `promptCapabilities.image: true` for
                // the agent as a whole, so the tray is offered on every row. A
                // model that cannot take one refuses at the send, which is
                // Cline's own answer to give.
                accepts_images: true,
                // Every row is secondary: 308 models is not a list Shift+Tab
                // can walk, so the top level is the reader's starred shortlist
                // and this list is what the library dialog draws.
                secondary: true,
                supports_fast: false,
                free: is_free(&row.model_id),
            }
        })
        .collect()
}

/// Whether Cline serves this model for nothing.
///
/// The suffix is OpenRouter's and Cline passes it through on both halves of a
/// row — `google/gemma-4-31b-it:free`, `Gemma 4 31B (free)` — so the id is
/// read and the name is not: a name is display text somebody may reword, and
/// the word "free" appears in plenty of prose.
///
/// Deliberately under-matching. `openrouter/free` is a *router* over free
/// models rather than a model with a free tier, and nothing on the wire says
/// what it resolves to — so it draws no mark rather than a promise this build
/// cannot keep.
fn is_free(model_id: &str) -> bool {
    model_id.ends_with(":free")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `session/new` reply, model list trimmed to three rows.
    const SESSION_NEW: &str = include_str!("fixtures/session_new.json");

    fn captured() -> Vec<Model> {
        let reply: Value = serde_json::from_str(SESSION_NEW).expect("fixture is JSON");
        models_of(&reply["result"])
    }

    /// The display name is the point of reading this reply at all.
    #[test]
    fn a_row_keeps_clines_own_name() {
        let models = captured();

        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id.as_str(), "xiaomi/mimo-v2.6-flash");
        assert_eq!(models[0].label, "MiMo-V2.6-Flash");
        assert_eq!(models[0].arg, "xiaomi/mimo-v2.6-flash");
    }

    /// The provider is the id's first segment, with Cline's own routing marker
    /// cut — `~openai/gpt-sol-latest` is OpenAI's, and a heading reading
    /// `~openai` beside another reading `openai` is one provider drawn twice.
    #[test]
    fn the_provider_is_the_first_segment_without_the_routing_mark() {
        let models = models_of(&json!({
            "models": {"availableModels": [
                {"modelId": "~openai/gpt-sol-latest", "name": "GPT Sol Latest"},
                {"modelId": "x-ai/grok-4.7", "name": "Grok 4.7"},
                {"modelId": "bare", "name": "Bare"}
            ]}
        }));

        assert_eq!(models[0].provider, "openai");
        assert_eq!(models[1].provider, "x-ai");
        assert_eq!(models[2].provider, "cline", "an id with no slash is Cline's own");
    }

    /// No level exists to send, so none may be offered — a rung drawn here
    /// would be one Cline has nowhere to put.
    #[test]
    fn no_model_offers_an_effort_level() {
        assert!(captured().iter().all(|m| m.efforts.is_empty()));
    }

    /// A reply with no models at all is an ordinary answer, not an error: it is
    /// what a session opened before a provider was picked reports.
    #[test]
    fn a_reply_with_no_models_answers_an_empty_list() {
        assert!(models_of(&json!({})).is_empty());
        assert!(models_of(&json!({"models": {"availableModels": []}})).is_empty());
    }

    /// Cline marks its free rows on the id, and that mark is the only signal
    /// on the wire — no row carries a price.
    #[test]
    fn a_free_row_is_marked_off_its_id() {
        let models = models_of(&json!({
            "models": {"availableModels": [
                {"modelId": "google/gemma-4-31b-it:free", "name": "Gemma 4 31B (free)"},
                {"modelId": "anthropic/claude-sonnet-5", "name": "Claude Sonnet 5"},
                {"modelId": "openrouter/free", "name": "Free Models Router"},
                {"modelId": "x-ai/grok-4.7", "name": "Grok 4.7 (free trial)"}
            ]}
        }));

        assert!(models[0].free);
        assert!(!models[1].free);
        assert!(
            !models[2].free,
            "a router over free models is not itself a model with a free tier"
        );
        assert!(
            !models[3].free,
            "the name is display text, so only the id decides"
        );
    }
}
