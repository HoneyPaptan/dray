//! Cline's ACP wire format, typed.
//!
//! ACP over stdio again, the fourth of that shape here, so the envelope arrives
//! split into a method and a params object by
//! [`codex::rpc`](crate::harness::codex::rpc) and this only names what is
//! inside. Same conventions as the parsers beside it: every enum that can grow
//! carries `#[serde(other)]`, fields the server may omit carry
//! `#[serde(default)]`, and a shape not modelled costs one field or one line,
//! never the connection.
//!
//! Every shape here was read off a live `cline --acp` (3.0.64), captured in
//! `fixtures/`. Three things it does that opencode does not, each measured:
//!
//! - **`session/new` answers `modes` and `models` beside `configOptions`**, and
//!   the model rows there carry a display *name* where the config option's
//!   carry only a value. That is the whole reason the picker reads this reply
//!   rather than a `cline models` there is no subcommand for.
//! - **`config_option_update` is a streamed update**, so a model or mode moved
//!   by the reader's own client reaches Dray without being asked for.
//! - **There is no usage of any kind on the wire.** No `usage_update`, no
//!   token count on the prompt response, nothing under `_meta` — so the
//!   composer's context ring has no source and is drawn empty. Pinned by test,
//!   since a later version growing one should be noticed rather than kept
//!   absent by habit. What Cline *does* record is on disk, and spend is read
//!   from there instead — see [`super::usage`].

use serde::Deserialize;
use serde_json::Value;

/// Reads `null` as the type's default, which `#[serde(default)]` alone will not.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// A line the mapper acts on, or a marker saying why it does not.
pub enum ClineEvent {
    Update(SessionUpdate),
    /// The answer to `session/prompt`, which is how a turn ends: there is no
    /// turn-completed notification, the request itself blocks for the turn.
    PromptDone(PromptResponse),
    /// `session/prompt` answered with a JSON-RPC error — an expired login, a
    /// model the provider will not serve. The turn never opened.
    PromptFailed { message: String },
    /// A method this build has never seen. Filed, and costs nothing else.
    Unknown,
}

/// `session/update`'s params.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotification {
    #[serde(default)]
    pub session_id: String,
    pub update: SessionUpdate,
}

/// One streamed update, tagged on `sessionUpdate`.
///
/// The unit variants are updates seen and drawn as nothing: `user_message_chunk`
/// is the `session/load` replay of a prompt Dray already logged, and
/// `current_mode_update` echoes a stance this build just set. The distinction
/// from [`Self::Unknown`] is what keeps the failure log a signal.
#[derive(Debug, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum SessionUpdate {
    #[serde(rename_all = "camelCase")]
    AgentMessageChunk {
        #[serde(default)]
        message_id: Option<String>,
        content: ContentBlock,
    },
    AgentThoughtChunk {
        content: ContentBlock,
    },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        tool_call_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        kind: ToolKind,
        #[serde(default)]
        status: ToolStatus,
        #[serde(default)]
        raw_input: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    ToolCallUpdate {
        tool_call_id: String,
        #[serde(default)]
        status: Option<ToolStatus>,
        #[serde(default, deserialize_with = "null_as_default")]
        content: Vec<ToolContent>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        kind: Option<ToolKind>,
        #[serde(default)]
        raw_input: Option<Value>,
        #[serde(default)]
        raw_output: Option<Value>,
    },
    /// Every option restated whenever any one of them moves — including by the
    /// reader's own client, which is why this is read rather than assumed.
    #[serde(rename_all = "camelCase")]
    ConfigOptionUpdate {
        #[serde(default)]
        config_options: Vec<ConfigOption>,
    },
    SessionInfoUpdate {
        #[serde(default)]
        title: Option<String>,
    },
    UserMessageChunk,
    CurrentModeUpdate,
    Plan,
    #[serde(other)]
    Unknown,
}

/// An ACP content block. Only text is drawn; an image or resource block is
/// kept from failing the line and drawn as nothing.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    #[serde(other)]
    Other,
}

impl ContentBlock {
    pub fn text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            ContentBlock::Other => None,
        }
    }
}

/// What a tool call reports back, tagged on `type`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Content {
        content: ContentBlock,
    },
    #[serde(rename_all = "camelCase")]
    Diff {
        #[serde(default)]
        path: String,
        #[serde(default)]
        old_text: Option<String>,
        #[serde(default)]
        new_text: String,
    },
    #[serde(other)]
    Other,
}

/// ACP's closed set of tool kinds, which is what lets a call be classified
/// without a table of Cline's own tool names.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
    #[serde(other)]
    Unknown,
}

impl ToolStatus {
    /// Whether this update closes the call.
    pub fn is_final(self) -> bool {
        matches!(self, ToolStatus::Completed | ToolStatus::Failed)
    }
}

/// `session/request_permission`'s params. The options are the server's and go
/// back as they came — see [`permissions`](super::permissions).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub tool_call: ToolCallRef,
    #[serde(default)]
    pub options: Vec<PermissionChoice>,
}

/// The call a permission request names.
///
/// Cline's `title` here is the tool's name with its arguments after it
/// (`run_commands: pwd && ls -la`), so the card's subject is read out of
/// `rawInput` instead — the row above it already says the tool.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRef {
    #[serde(default)]
    pub tool_call_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub kind: ToolKind,
    #[serde(default)]
    pub raw_input: Option<Value>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionChoice {
    pub option_id: String,
    #[serde(default)]
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always`. A string
    /// rather than an enum so a kind added later reaches
    /// [`permissions`](super::permissions)'s own fallback instead of failing
    /// the request.
    #[serde(default)]
    pub kind: String,
}

/// The `session/prompt` response.
///
/// `stopReason` and nothing else — **no usage**, measured against 3.0.64, where
/// ACP's schema allows one and opencode fills it. Adding a `usage` field here
/// would be this build claiming a number nothing sends.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    /// `end_turn`, `cancelled`, `refused`, `max_tokens`, `max_turn_requests`.
    #[serde(default)]
    pub stop_reason: String,
}

/// What `session/new` answers with.
///
/// Three groups, and the models one is why this type exists at all: it is the
/// only place on the wire a model's **display name** appears, `configOptions`
/// carrying bare values. See [`models`](super::models).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOpened {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub models: AvailableModels,
    #[serde(default)]
    pub config_options: Vec<ConfigOption>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModels {
    #[serde(default)]
    pub available_models: Vec<ModelRow>,
    #[serde(default)]
    pub current_model_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub name: String,
}

/// One session setting. Four exist, measured: `provider`, `model`, `mode` and
/// `auto_approve` — and **no `effort`**, where the `cline` CLI's own
/// `--thinking` flag suggests one. A level is not reachable over ACP, so none
/// is learned, drawn or sent.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    #[serde(default)]
    pub id: String,
    /// `select` or `boolean`, and the setter has to echo it back — a
    /// `set_config_option` carrying the wrong one is refused with
    /// `expected "boolean"` and nothing about the option being set.
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub current_value: Option<Value>,
    #[serde(default)]
    pub options: Vec<ConfigChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigChoice {
    #[serde(default)]
    pub value: String,
}

/// The settings a session carries, as answered by `session/new`,
/// `session/load`, every `session/set_config_option` and the streamed
/// `config_option_update` alike.
#[derive(Debug, Default)]
pub struct ConfigOptions {
    pub config_options: Vec<ConfigOption>,
}

impl ConfigOptions {
    /// Reads the list off a JSON-RPC `result`. Absent or misshapen answers an
    /// empty list, which every reader here takes as "nothing was said" rather
    /// than as a statement about the session.
    pub fn of(result: &Value) -> Self {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            #[serde(default)]
            config_options: Vec<ConfigOption>,
        }
        let wire: Wire = serde_json::from_value(result.clone()).unwrap_or_default();
        Self {
            config_options: wire.config_options,
        }
    }

    /// What a named option is currently set to, as a string.
    ///
    /// Read rather than assumed, because it is the only statement of what the
    /// session is *running*: a model this build sent may have been refused, and
    /// the reader's own client may have moved one underneath it.
    pub fn current(&self, id: &str) -> Option<String> {
        let value = self
            .config_options
            .iter()
            .find(|o| o.id == id)?
            .current_value
            .as_ref()?;
        match value {
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }

    /// The `type` a named option declares, which its setter must echo.
    pub fn kind(&self, id: &str) -> Option<&str> {
        self.config_options
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.kind.as_str())
    }

    /// Every value a named option offers.
    pub fn values(&self, id: &str) -> Vec<&str> {
        self.config_options
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.options.iter().map(|c| c.value.as_str()).collect())
            .unwrap_or_default()
    }
}

/// Types one notification off the connection.
pub fn parse_notification(method: &str, params: Value) -> Result<ClineEvent, serde_json::Error> {
    Ok(match method {
        "session/update" => {
            let update: UpdateNotification = serde_json::from_value(params)?;
            ClineEvent::Update(update.update)
        }
        _ => ClineEvent::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One real session: the `session/load` replay of an earlier turn, then a
    /// turn that ran three tools and wrote a file.
    const LIVE_TURN: &str = include_str!("fixtures/live_turn.jsonl");
    /// The reply `session/new` answers with, model list trimmed to three rows.
    const SESSION_NEW: &str = include_str!("fixtures/session_new.json");

    fn updates(fixture: &str) -> Vec<SessionUpdate> {
        fixture
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<Value>(line).expect("fixture line is JSON"))
            .filter(|v| v.get("method").and_then(Value::as_str) == Some("session/update"))
            .map(|v| {
                let n: UpdateNotification =
                    serde_json::from_value(v["params"].clone()).expect("update parses");
                n.update
            })
            .collect()
    }

    /// Nothing in a captured session may land on [`SessionUpdate::Unknown`]: an
    /// update kind filed there is one this build learned nothing from, and the
    /// distinction from a *named* unit variant is what keeps the failure log a
    /// signal.
    #[test]
    fn every_update_in_a_real_session_is_named() {
        let updates = updates(LIVE_TURN);

        assert!(!updates.is_empty());
        assert!(!updates.iter().any(|u| matches!(u, SessionUpdate::Unknown)));
    }

    /// The four options, and the absence of a fifth is the load-bearing half:
    /// an `effort` appearing here would mean this harness had a ladder to draw.
    #[test]
    fn a_session_publishes_four_options_and_no_effort() {
        let reply: Value = serde_json::from_str(SESSION_NEW).expect("fixture is JSON");
        let config = ConfigOptions::of(&reply["result"]);

        let ids: Vec<&str> = config.config_options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["provider", "model", "mode", "auto_approve"]);
        assert_eq!(config.values("mode"), ["plan", "act"]);
        assert_eq!(config.current("effort"), None);
    }

    /// The setter has to echo an option's own `type`, so reading it has to
    /// work for the boolean as well as the selects.
    #[test]
    fn an_option_reports_the_type_its_setter_must_echo() {
        let reply: Value = serde_json::from_str(SESSION_NEW).expect("fixture is JSON");
        let config = ConfigOptions::of(&reply["result"]);

        assert_eq!(config.kind("model"), Some("select"));
        assert_eq!(config.kind("auto_approve"), Some("boolean"));
        // A boolean's current value is not a string, and reading it as one is
        // what the display half needs.
        assert_eq!(config.current("auto_approve").as_deref(), Some("false"));
    }

    /// The model rows are the only place a display name exists — the `model`
    /// config option carries bare values — which is the whole reason the picker
    /// reads this reply.
    #[test]
    fn session_new_carries_named_models() {
        let reply: Value = serde_json::from_str(SESSION_NEW).expect("fixture is JSON");
        let opened: SessionOpened =
            serde_json::from_value(reply["result"].clone()).expect("the reply parses");

        assert!(!opened.session_id.is_empty());
        assert_eq!(opened.models.available_models[0].model_id, "xiaomi/mimo-v2.6-flash");
        assert_eq!(opened.models.available_models[0].name, "MiMo-V2.6-Flash");
        assert!(opened.models.current_model_id.is_some());
    }

    /// Nothing on this wire reports tokens. Pinned so a version that grows a
    /// usage update is noticed rather than left unread out of habit.
    #[test]
    fn a_captured_session_reports_no_usage_at_all() {
        assert!(!LIVE_TURN.contains("usage_update"));
        assert!(!LIVE_TURN.contains("totalTokens"));

        let done: Vec<PromptResponse> = LIVE_TURN
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v.get("result").cloned())
            .filter(|r| r.get("stopReason").is_some())
            .map(|r| serde_json::from_value(r).expect("prompt response parses"))
            .collect();

        assert_eq!(done.len(), 1);
        assert_eq!(done[0].stop_reason, "end_turn");
    }
}
