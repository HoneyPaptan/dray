//! opencode's ACP wire format, typed.
//!
//! ACP (Agent Client Protocol) is newline JSON-RPC 2.0 over stdio, the shape
//! `codex app-server` already has, so the envelope arrives split into a method
//! and a params object by [`codex::rpc`](crate::harness::codex::rpc) and this
//! only names what is inside. Same conventions as the other parsers: every enum
//! that can grow carries `#[serde(other)]`, fields the server may omit carry
//! `#[serde(default)]`, and a shape not modelled costs one field or one line,
//! never the connection.
//!
//! Every shape here was read off a live `opencode acp` (1.18.30), captured in
//! `fixtures/`, not off the ACP schema — and not off fx's capture either,
//! though fx speaks the same dialect. Three places they differ, each one a
//! field this build would otherwise read as absent for ever: a finished tool
//! answers `rawOutput` where fx sends `command_result`, a shell's exit status
//! is `metadata.exit` rather than an `exitCode`, and `usage_update` carries a
//! priced `cost` beside the occupancy.

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
pub enum OpencodeEvent {
    Update(SessionUpdate),
    /// The answer to `session/prompt`, which is how a turn ends: there is no
    /// turn-completed notification, the request itself blocks for the turn.
    PromptDone(PromptResponse),
    /// `session/prompt` answered with a JSON-RPC error — a provider refusing
    /// the login, a model no provider serves. The turn never opened.
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
/// is the `session/load` replay of a prompt Dray already logged,
/// `available_commands_update` arrived empty on every capture. The distinction
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
        name: Option<String>,
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
        /// Restated on the update, and worth reading: the opening `tool_call`
        /// names the *tool* (`bash`), where the update names the work
        /// (`echo hi > /tmp/oc/a.txt`). Measured — see `fixtures/live_turn.jsonl`.
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        kind: Option<ToolKind>,
        /// **The arguments arrive here, not on the opening call.** A `bash`
        /// call opens carrying `{"cwd": …}` and the command lands on the next
        /// update — see the mapper's `announce`, which is the whole reason a
        /// row is held rather than drawn twice.
        #[serde(default)]
        raw_input: Option<Value>,
        /// The tool's own result, where fx sends a `command_result` beside the
        /// ACP fields. A shell's exit code lives at `metadata.exit`.
        #[serde(default)]
        raw_output: Option<RawOutput>,
    },
    SessionInfoUpdate {
        #[serde(default)]
        title: Option<String>,
    },
    /// An occupancy reading — `used` of `size` — not a cumulative. The trap
    /// Codex's `total` and Claude's `result.usage` both set is absent here.
    UsageUpdate {
        #[serde(default)]
        used: Option<u64>,
        #[serde(default)]
        size: Option<u64>,
        /// What the session has cost so far, which opencode prices itself and
        /// no other ACP harness sends. A running total, not a delta.
        #[serde(default)]
        cost: Option<Cost>,
    },
    #[serde(rename_all = "camelCase")]
    AvailableCommandsUpdate {
        #[serde(default)]
        available_commands: Vec<AvailableCommand>,
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
///
/// `diff` is in the ACP schema and no capture carried one — the editor names
/// its sides in `rawInput` instead — so it is modelled to keep the line and
/// read by nothing yet.
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

/// ACP's closed set of tool kinds, which is what makes classifying a call
/// possible without knowing opencode's tool names.
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

/// What a finished tool handed back, opencode's own field beside the ACP ones.
///
/// `output` is the same text the `content` blocks carry; what is only here is
/// `metadata`, and the one field read off it is a shell's exit code. Kept as a
/// typed struct rather than a `Value` so a shape that moves fails one field
/// rather than a whole line.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawOutput {
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub metadata: OutputMetadata,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputMetadata {
    /// A shell's exit status. `exit`, not `exitCode`.
    #[serde(default)]
    pub exit: Option<i64>,
    #[serde(default)]
    pub truncated: Option<bool>,
}

/// A running cost, as opencode prices it.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
pub struct Cost {
    #[serde(default)]
    pub amount: f64,
}

/// One command opencode published for the `/` picker.
///
/// `name` and `description` are all it sends — there is no input hint on the
/// wire at all — so the hint is modelled to keep a later one from failing the
/// line rather than because anything fills it today.
#[derive(Debug, Clone, Deserialize)]
pub struct AvailableCommand {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input: Option<CommandInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandInput {
    #[serde(default)]
    pub hint: Option<String>,
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

/// The call a permission request names. A subset of `tool_call`'s fields, and
/// `rawInput` is what the card draws the command or path from.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRef {
    #[serde(default)]
    pub tool_call_id: String,
    #[serde(default)]
    pub name: Option<String>,
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
    /// rather than an enum so a kind added later reaches [`permissions`]'s
    /// own fallback instead of failing the request.
    #[serde(default)]
    pub kind: String,
}

/// The `session/prompt` response.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    /// `end_turn`, `cancelled`, `refused`, `max_tokens`, `max_turn_requests`.
    #[serde(default)]
    pub stop_reason: String,
    #[serde(default)]
    pub usage: PromptUsage,
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

/// The settings a session carries, answered by `session/new`, `session/load`
/// and every `session/set_config_option` alike.
///
/// Two options and no others, measured: `model`, carrying every model the
/// reader's providers serve, and `mode`, carrying `build` and `plan`. **There
/// is no `effort`** — where fx publishes a per-model ladder here, opencode
/// publishes none at all, which is why nothing in this harness learns, draws or
/// sends a reasoning level.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptions {
    #[serde(default)]
    pub config_options: Vec<ConfigOption>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub current_value: Option<String>,
    #[serde(default)]
    pub options: Vec<ConfigChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigChoice {
    #[serde(default)]
    pub value: String,
}

impl ConfigOptions {
    /// Reads the list off a JSON-RPC `result`. Absent or misshapen answers an
    /// empty list, which every reader here takes as "nothing was said" rather
    /// than as a statement about the session.
    pub fn of(result: &Value) -> Self {
        serde_json::from_value(result.clone()).unwrap_or_default()
    }

    /// What a named option is currently set to — `model` or `mode`, which is
    /// the whole list opencode publishes.
    ///
    /// Read rather than assumed, because it is the only statement of what the
    /// session is *running*: a model this build sent may have been refused, and
    /// a mode may be one the reader's own config moved.
    pub fn current(&self, id: &str) -> Option<&str> {
        self.config_options
            .iter()
            .find(|o| o.id == id)?
            .current_value
            .as_deref()
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
pub fn parse_notification(method: &str, params: Value) -> Result<OpencodeEvent, serde_json::Error> {
    Ok(match method {
        "session/update" => {
            let update: UpdateNotification = serde_json::from_value(params)?;
            OpencodeEvent::Update(update.update)
        }
        _ => OpencodeEvent::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One real turn: two tool calls, a message and the usage that closed it.
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

    /// Nothing in a captured turn may land on [`SessionUpdate::Unknown`]: an
    /// update kind filed there is one this build learned nothing from, and the
    /// distinction from a *named* unit variant is what keeps the failure log a
    /// signal.
    #[test]
    fn every_update_in_a_real_turn_is_named() {
        let updates = updates(LIVE_TURN);

        assert!(!updates.is_empty());
        assert!(!updates
            .iter()
            .any(|u| matches!(u, SessionUpdate::Unknown)));
    }

    /// The exit code is the field most likely to be read as absent for ever: it
    /// is opencode's own, nested under `rawOutput.metadata`, and spelled `exit`
    /// rather than `exitCode`. A rename here costs no error and no failed line
    /// — only a shell row that stops saying whether the command worked.
    #[test]
    fn a_finished_shell_carries_its_exit_status() {
        let closing = updates(LIVE_TURN)
            .into_iter()
            .find_map(|u| match u {
                SessionUpdate::ToolCallUpdate {
                    status: Some(ToolStatus::Completed),
                    raw_output: Some(out),
                    ..
                } if out.metadata.exit.is_some() => Some(out),
                _ => None,
            })
            .expect("the capture closes a shell call");

        assert_eq!(closing.metadata.exit, Some(0));
    }

    /// The opening `tool_call` names the tool and the update names the work, so
    /// a row built from the first alone says "bash" for every command there is.
    #[test]
    fn an_update_restates_the_title_with_the_work_in_it() {
        let titles: Vec<String> = updates(LIVE_TURN)
            .into_iter()
            .filter_map(|u| match u {
                SessionUpdate::ToolCallUpdate { title, .. } => title,
                _ => None,
            })
            .collect();

        assert!(titles.iter().any(|t| t == "echo hi > /tmp/oc/a.txt"));
    }

    /// The occupancy is a reading, not a cumulative — and the cost beside it is
    /// the opposite, which is why they are separate fields rather than one
    /// "usage" the mapper has to guess about.
    #[test]
    fn usage_is_an_occupancy_with_a_price_beside_it() {
        let usage = updates(LIVE_TURN)
            .into_iter()
            .find_map(|u| match u {
                SessionUpdate::UsageUpdate { used, size, cost } => Some((used, size, cost)),
                _ => None,
            })
            .expect("the capture reports usage");

        assert_eq!(usage.0, Some(14971));
        assert_eq!(usage.1, Some(1048576));
        assert_eq!(usage.2.map(|c| c.amount), Some(0.0));
    }

    /// Two options, and the absence of a third is the load-bearing half: an
    /// `effort` appearing here would mean this harness had a ladder to draw.
    #[test]
    fn a_session_publishes_a_model_and_a_mode_and_nothing_else() {
        let reply: Value = serde_json::from_str(SESSION_NEW).expect("fixture is JSON");
        let config = ConfigOptions::of(&reply["result"]);

        let ids: Vec<&str> = config.config_options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["model", "mode"]);
        assert_eq!(config.values("mode"), ["build", "plan"]);
        assert_eq!(config.current("model"), Some("opencode/big-pickle"));
        assert_eq!(config.current("effort"), None);
    }
}
