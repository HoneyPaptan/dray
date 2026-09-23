//! Cline's ACP vocabulary onto Dray's.
//!
//! Three things are synthesized rather than read, the same three every ACP
//! harness here synthesizes. Cline sends no turn-started line — the turn opens
//! when the prompt request is written and closes when it answers — so
//! `TurnStarted` is minted on the first update after a prompt. It sends no
//! "requesting" ping, so `ModelRequestStarted` is minted with it and after
//! every tool result. And a thought chunk carries no id, so one thinking block
//! runs until the next non-thought update.
//!
//! What it does *not* synthesize is a context reading. Cline reports no tokens
//! on the wire — see the parser — and a number invented here would be one the
//! composer's ring then draws as measurement. Nor does it fill the turn's
//! spend: that is read off Cline's own session file by [`super::usage`], which
//! is the layer that can touch a disk.

use crate::events::{
    AgentEvent, AgentEventPayload, BlockRef, BlockType, DeltaEvent, SessionInfo, ToolResult,
    ToolType, TurnStatus,
};
use crate::harness::{mentions_any, Harness};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

use super::parser::{
    ClineEvent, PromptResponse, SessionUpdate, ToolContent, ToolKind, ToolStatus,
};

/// What Cline says when its login has gone. Written to under-match: a wording
/// missed costs the login button and keeps the sentence. Read off the live
/// refusal `Authentication required: Call authenticate before starting a
/// session`.
const LOGIN_NEEDLES: &[&str] = &[
    "authentication required",
    "not authenticated",
    "log in",
    "login",
    "sign in",
];

/// A streamed block still open, and the text it has accumulated so far — the
/// committed event supersedes the deltas, so the whole text is kept.
struct OpenBlock {
    id: String,
    kind: BlockType,
    text: String,
}

/// A call announced by `tool_call` and not yet drawn.
struct Opening {
    name: String,
    kind: ToolKind,
    input: Option<Value>,
}

/// Per-session state the mapping needs across lines.
pub struct Mapper {
    /// Dray's own id, never Cline's. Every event the frontend routes is keyed
    /// on this, and the two are only joined on the index entry.
    session_id: String,
    seq: Arc<AtomicU64>,
    turn_open: bool,
    /// The one block streaming right now. Thought chunks carry no ids, so at
    /// most one block is open and a chunk of the other kind closes it.
    open: Option<OpenBlock>,
    /// Ids handed to thought blocks, which carry none of their own.
    thoughts: u64,
    /// Text a running call has streamed, by call id, for the calls whose
    /// closing update carries nothing.
    outputs: HashMap<String, String>,
    /// Calls opened but not yet announced, by id.
    opening: HashMap<String, Opening>,
    /// Whether a `session/load` replay is still being read. Cline replays the
    /// whole conversation on a resume and Dray already holds every line of it,
    /// so the replay is dropped rather than logged twice.
    replaying: bool,
}

impl Mapper {
    pub fn new(session_id: String, seq: Arc<AtomicU64>) -> Self {
        Self {
            session_id,
            seq,
            turn_open: false,
            open: None,
            thoughts: 0,
            outputs: HashMap::new(),
            opening: HashMap::new(),
            replaying: false,
        }
    }

    /// Drops everything until the next prompt.
    ///
    /// `session/load` replays the conversation as ordinary updates — the same
    /// `agent_message_chunk`s the first turn sent — and Dray's log already
    /// holds every one of them, so replaying them into it would double the
    /// transcript on every resume. Armed by the load and disarmed by the next
    /// prompt going out, which is the only thing that can tell the replay from
    /// live work.
    pub fn begin_replay(&mut self) {
        self.replaying = true;
    }

    /// Ends the replay window. Called when a prompt is written, since from
    /// there every update is this session's own work.
    pub fn end_replay(&mut self) {
        self.replaying = false;
    }

    pub fn map(&mut self, event: ClineEvent) -> Vec<AgentEvent> {
        if self.replaying {
            return Vec::new();
        }
        match event {
            ClineEvent::Update(update) => self.update(update),
            ClineEvent::PromptDone(response) => self.prompt_done(response),
            ClineEvent::PromptFailed { message } => self.prompt_failed(message),
            ClineEvent::Unknown => Vec::new(),
        }
    }

    fn update(&mut self, update: SessionUpdate) -> Vec<AgentEvent> {
        match update {
            SessionUpdate::AgentMessageChunk {
                message_id,
                content,
            } => {
                let Some(text) = content.text() else {
                    return Vec::new();
                };
                let id = message_id.unwrap_or_else(|| "message".to_string());
                let mut out = self.ensure_turn();
                out.extend(self.stream(id, BlockType::Text, text));
                out
            }

            SessionUpdate::AgentThoughtChunk { content } => {
                let Some(text) = content.text() else {
                    return Vec::new();
                };
                let mut out = self.ensure_turn();
                let id = match &self.open {
                    Some(block) if block.kind == BlockType::Thinking => block.id.clone(),
                    _ => {
                        self.thoughts += 1;
                        format!("thought-{}", self.thoughts)
                    }
                };
                out.extend(self.stream(id, BlockType::Thinking, text));
                out
            }

            SessionUpdate::ToolCall {
                tool_call_id,
                title,
                kind,
                raw_input,
                ..
            } => {
                // Held, not drawn — opencode's bargain, for a different reason.
                // There the opening call carries no arguments; here it carries
                // them but its `title` is the tool's name with the whole
                // argument blob pasted after it (`editor: {"path":…}`), which
                // is not a name any row should draw. Holding lets the name be
                // cut once, off the one update that has both.
                self.opening.insert(
                    tool_call_id,
                    Opening {
                        name: tool_name(title.as_deref(), kind),
                        kind,
                        input: raw_input,
                    },
                );
                Vec::new()
            }

            SessionUpdate::ToolCallUpdate {
                tool_call_id,
                status,
                content,
                title,
                kind,
                raw_input,
                raw_output,
            } => {
                let text: String = content
                    .iter()
                    .filter_map(|c| match c {
                        ToolContent::Content { content } => content.text(),
                        _ => None,
                    })
                    .collect();

                if let Some(held) = self.opening.get_mut(&tool_call_id) {
                    if let Some(kind) = kind {
                        held.kind = kind;
                    }
                    if let Some(input) = raw_input {
                        if !is_thin(&input) {
                            held.input = Some(input);
                        }
                    }
                    if let Some(title) = title.as_deref() {
                        let named = tool_name(Some(title), held.kind);
                        if !named.is_empty() {
                            held.name = named;
                        }
                    }
                }

                let final_status = status.filter(|s| s.is_final());
                let mut out = self.announce(&tool_call_id, final_status.is_some());

                let Some(status) = final_status else {
                    if !text.is_empty() {
                        self.outputs
                            .entry(tool_call_id)
                            .or_default()
                            .push_str(&text);
                    }
                    return out;
                };

                let streamed = self.outputs.remove(&tool_call_id).unwrap_or_default();
                let result = ToolResult {
                    text: result_text(streamed, text, &raw_output),
                    is_error: status == ToolStatus::Failed,
                    structured: None,
                    // Nothing on this wire reports an exit status: a shell's
                    // result arrives as content, and `rawOutput` on the capture
                    // carries the same text rather than a code beside it.
                    exit_code: None,
                    duration_ms: None,
                    images: Vec::new(),
                };

                out.push(self.event(AgentEventPayload::ToolCallCompleted {
                    call_id: tool_call_id.clone(),
                    result,
                }));
                // The model reads the result next. Same reading Codex's mapper
                // makes, for the same working indicator.
                out.push(self.event(AgentEventPayload::ModelRequestStarted));
                out
            }

            // Read by the read loop off the parsed update, not mapped: a
            // settings change is a fact about the index row and the composer,
            // not a transcript event.
            SessionUpdate::ConfigOptionUpdate { .. } => Vec::new(),

            // Likewise a title: a fact about the index row.
            SessionUpdate::SessionInfoUpdate { .. } => Vec::new(),

            SessionUpdate::UserMessageChunk
            | SessionUpdate::CurrentModeUpdate
            | SessionUpdate::Plan
            | SessionUpdate::Unknown => Vec::new(),
        }
    }

    /// Draws a held call's row, once there is something worth drawing.
    ///
    /// Announced when the arguments are worth a row or when the call closes,
    /// whichever lands first, so a call is drawn exactly once and always with
    /// the best input seen.
    fn announce(&mut self, call_id: &str, closing: bool) -> Vec<AgentEvent> {
        let Some(held) = self.opening.get(call_id) else {
            return Vec::new();
        };
        let has_input = held.input.as_ref().is_some_and(|i| !is_thin(i));
        if !has_input && !closing {
            return Vec::new();
        }

        let held = self.opening.remove(call_id).expect("held above");
        let mut out = self.ensure_turn();
        out.extend(self.close_open());
        out.push(self.event(AgentEventPayload::ToolCallStarted {
            call_id: call_id.to_string(),
            tool_type: tool_type(held.kind),
            name: held.name,
            input: tool_input(held.input),
            raw_input: None,
            // The row draws the work off the input, and Cline's own title is
            // the tool name with that same input pasted after it.
            title: None,
        }));
        out
    }

    fn prompt_done(&mut self, response: PromptResponse) -> Vec<AgentEvent> {
        let mut out = self.close_open();

        let (status, final_text) = match response.stop_reason.as_str() {
            "refused" | "refusal" => (
                TurnStatus::Error,
                Some("Cline refused this prompt.".to_string()),
            ),
            "max_tokens" => (
                TurnStatus::Error,
                Some("Cline stopped: the model hit its output token limit.".to_string()),
            ),
            "max_turn_requests" => (
                TurnStatus::Error,
                Some("Cline stopped: the turn hit its request limit.".to_string()),
            ),
            // `end_turn`, and `cancelled` — the reader's own Stop, reported as
            // a success carrying a reason nothing draws.
            _ => (TurnStatus::Success, None),
        };

        out.push(self.turn_completed(status, Some(response.stop_reason), final_text, false));
        out
    }

    /// `session/prompt` refused outright. The sentence is Cline's own and
    /// usually names its cure, so it is the row's text.
    fn prompt_failed(&mut self, message: String) -> Vec<AgentEvent> {
        let mut out = self.close_open();
        let auth_failed = mentions_any(&message, LOGIN_NEEDLES);
        out.push(self.turn_completed(TurnStatus::Error, None, Some(message), auth_failed));
        out
    }

    fn turn_completed(
        &mut self,
        status: TurnStatus,
        stop_reason: Option<String>,
        final_text: Option<String>,
        auth_failed: bool,
    ) -> AgentEvent {
        let event = self.event(AgentEventPayload::TurnCompleted {
            status,
            stop_reason,
            auth_failed,
            final_text,
            // Filled by the read loop, the only layer that can read Cline's
            // own session file. Nothing on the wire carries a figure.
            usage: None,
            duration_ms: None,
            // Filled by `session::ingest`, the only layer that knows the tree.
            head: None,
        });
        self.turn_open = false;
        self.outputs.clear();
        event
    }

    /// Opens the turn on its first update.
    fn ensure_turn(&mut self) -> Vec<AgentEvent> {
        if self.turn_open {
            return Vec::new();
        }
        self.turn_open = true;
        vec![
            self.event(AgentEventPayload::TurnStarted(SessionInfo {
                cwd: None,
                model: None,
                harness_version: None,
                tools: Vec::new(),
                mcp_servers: Vec::new(),
                subagent_types: Vec::new(),
                settings: None,
            })),
            self.event(AgentEventPayload::ModelRequestStarted),
        ]
    }

    /// Appends a chunk to the block `id`, opening it first where it is not the
    /// one already open.
    fn stream(&mut self, id: String, kind: BlockType, text: &str) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        let same = self
            .open
            .as_ref()
            .is_some_and(|block| block.id == id && block.kind == kind);
        if !same {
            out.extend(self.close_open());
            out.push(self.event(AgentEventPayload::Delta(DeltaEvent::BlockStart {
                block: block_ref(&id),
                block_type: kind.clone(),
            })));
            self.open = Some(OpenBlock {
                id: id.clone(),
                kind,
                text: String::new(),
            });
        }
        if let Some(block) = &mut self.open {
            block.text.push_str(text);
        }
        out.push(self.event(AgentEventPayload::Delta(DeltaEvent::TextDelta {
            block: block_ref(&id),
            text: text.to_string(),
        })));
        out
    }

    /// Closes the streaming block, committing its whole text: the deltas were a
    /// preview and this is what the transcript keeps.
    fn close_open(&mut self) -> Vec<AgentEvent> {
        let Some(block) = self.open.take() else {
            return Vec::new();
        };
        let stop = self.event(AgentEventPayload::Delta(DeltaEvent::BlockStop {
            block: block_ref(&block.id),
        }));
        let committed = match block.kind {
            BlockType::Thinking => self.event(AgentEventPayload::Reasoning {
                block: Some(block_ref(&block.id)),
                encrypted: block.text.is_empty(),
                text: block.text,
            }),
            _ => self.event(AgentEventPayload::AssistantText {
                block: Some(block_ref(&block.id)),
                text: block.text,
            }),
        };
        vec![stop, committed]
    }

    /// Mints an event the read loop needs but no update carried — a permission
    /// request arrives as a JSON-RPC *request* and never reaches [`Self::map`].
    pub fn synthesize(&self, payload: AgentEventPayload) -> AgentEvent {
        self.event(payload)
    }

    fn event(&self, payload: AgentEventPayload) -> AgentEvent {
        AgentEvent::mint(
            self.session_id.clone(),
            Harness::Cline,
            self.seq.fetch_add(1, Relaxed),
            // Cline names no turn on the wire, and minting one here would split
            // what the reader sees as one exchange.
            None,
            None,
            payload,
        )
    }
}

/// One message id is one block, so the index is always zero.
fn block_ref(id: &str) -> BlockRef {
    BlockRef {
        message_id: id.to_string(),
        index: 0,
    }
}

/// The tool's name out of Cline's title.
///
/// Its titles are `run_commands: pwd && ls -la` and
/// `editor: {"path":"/tmp/x","new_text":"HI"}` — the tool, a colon, then the
/// arguments. The name is what precedes the first colon, and only where what
/// precedes it looks like a tool name rather than prose: a title with no colon
/// at all is taken whole, and one whose head carries a space is dropped for the
/// kind, since that is a sentence rather than an identifier.
fn tool_name(title: Option<&str>, kind: ToolKind) -> String {
    let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) else {
        return kind_name(kind).to_string();
    };

    match title.split_once(':') {
        Some((head, _)) if !head.is_empty() && !head.contains(char::is_whitespace) => {
            head.to_string()
        }
        _ if !title.contains(char::is_whitespace) => title.to_string(),
        _ => kind_name(kind).to_string(),
    }
}

/// ACP's kind onto Dray's, which is what lets a tool Cline renames tomorrow
/// still draw as what it is.
fn tool_type(kind: ToolKind) -> ToolType {
    match kind {
        ToolKind::Read => ToolType::FileRead,
        ToolKind::Edit | ToolKind::Delete | ToolKind::Move => ToolType::FileEdit,
        ToolKind::Search => ToolType::Search,
        ToolKind::Execute => ToolType::Shell,
        ToolKind::Fetch => ToolType::Web,
        ToolKind::Think | ToolKind::SwitchMode | ToolKind::Other => ToolType::Other,
    }
}

/// A name for a call whose title said nothing usable.
fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "shell",
        ToolKind::Fetch => "fetch",
        ToolKind::Think => "think",
        ToolKind::SwitchMode => "switch_mode",
        ToolKind::Other => "tool",
    }
}

/// The call's arguments as the row draws them. Always an object.
///
/// Cline's shell takes a `commands` **array** where every other harness here
/// takes one string, and the row's summary reads `command` — so a single
/// command is flattened onto that key and a batch is left as the array it is.
fn tool_input(raw: Option<Value>) -> Value {
    let mut input = match raw {
        Some(Value::Object(map)) => Value::Object(map),
        Some(Value::Null) | None => json!({}),
        Some(other) => json!({ "_unparsed": other.to_string() }),
    };

    if let Some(map) = input.as_object_mut() {
        if let Some(Value::Array(commands)) = map.get("commands") {
            if let [Value::String(one)] = commands.as_slice() {
                let one = one.clone();
                map.remove("commands");
                map.insert("command".to_string(), Value::String(one));
            }
        }
    }
    input
}

/// What a finished call reports: its closing content, then whatever it
/// streamed, then `rawOutput` where it is a bare string.
fn result_text(streamed: String, closing: String, raw_output: &Option<Value>) -> String {
    if !closing.is_empty() {
        return closing;
    }
    if !streamed.is_empty() {
        return streamed;
    }
    match raw_output {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Whether a call's arguments say nothing worth drawing.
fn is_thin(input: &Value) -> bool {
    match input.as_object() {
        Some(map) => map.is_empty() || map.keys().all(|key| key == "cwd"),
        None => true,
    }
}

/// The tool a permission card is about, named without its arguments.
///
/// [`tool_name`]'s rule, exported: the card and the row have to agree about
/// what a call is called, and the request carries the same shape of title the
/// updates do.
pub fn tool_name_for_card(title: Option<&str>, kind: ToolKind) -> String {
    tool_name(title, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::AgentEventPayload as P;

    /// One real session: the resume replay, then a turn that ran a shell, an
    /// editor and a read, and wrote a file.
    const LIVE_TURN: &str = include_str!("fixtures/live_turn.jsonl");

    /// Replays a capture through the mapper: every `session/update`, then the
    /// prompt's own response, which is what ends a turn on this wire.
    fn replay(fixture: &str) -> Vec<AgentEvent> {
        let mut mapper = Mapper::new("s".into(), Arc::new(AtomicU64::new(0)));
        let mut out = Vec::new();

        for line in fixture.lines().filter(|l| !l.trim().is_empty()) {
            let value: Value = serde_json::from_str(line).expect("fixture line is JSON");

            if value.get("method").and_then(Value::as_str) == Some("session/update") {
                let update: super::super::parser::UpdateNotification =
                    serde_json::from_value(value["params"].clone()).expect("update parses");
                out.extend(mapper.map(ClineEvent::Update(update.update)));
                continue;
            }

            if let Some(result) = value.get("result").filter(|r| r.get("stopReason").is_some()) {
                let response: PromptResponse =
                    serde_json::from_value(result.clone()).expect("prompt response parses");
                out.extend(mapper.map(ClineEvent::PromptDone(response)));
            }
        }

        out
    }

    /// Every call is drawn exactly once, named after its tool rather than after
    /// the whole title — which on this wire is the tool with its arguments
    /// pasted after it.
    #[test]
    fn a_tool_row_is_named_after_its_tool() {
        let started: Vec<(String, Value)> = replay(LIVE_TURN)
            .into_iter()
            .filter_map(|e| match e.payload {
                P::ToolCallStarted { name, input, .. } => Some((name, input)),
                _ => None,
            })
            .collect();

        assert_eq!(started.len(), 3, "one row per call, announced exactly once");
        assert_eq!(started[0].0, "run_commands");
        assert_eq!(started[1].0, "editor");
        assert_eq!(started[2].0, "read_files");
    }

    /// The shell's `commands` array is flattened onto the key every row's
    /// summary reads, or a command row draws a tool with no command under it.
    #[test]
    fn a_single_shell_command_lands_on_the_summary_key() {
        let shell = replay(LIVE_TURN)
            .into_iter()
            .find_map(|e| match e.payload {
                P::ToolCallStarted { input, .. } if input.get("command").is_some() => Some(input),
                _ => None,
            })
            .expect("the capture runs a shell command");

        assert_eq!(
            shell.get("command").and_then(Value::as_str),
            Some("pwd && ls -la /tmp/clineprobe"),
        );
    }

    /// A resumed session replays its whole conversation as ordinary updates,
    /// and Dray's log already holds every line — so a replay drawn is the
    /// transcript doubled on every resume.
    #[test]
    fn a_replay_is_dropped_until_the_next_prompt() {
        let mut mapper = Mapper::new("s".into(), Arc::new(AtomicU64::new(0)));
        mapper.begin_replay();

        let chunk = SessionUpdate::AgentMessageChunk {
            message_id: None,
            content: serde_json::from_value(json!({"type": "text", "text": "old"}))
                .expect("content parses"),
        };
        assert!(mapper.map(ClineEvent::Update(chunk)).is_empty());

        mapper.end_replay();
        let live = SessionUpdate::AgentMessageChunk {
            message_id: None,
            content: serde_json::from_value(json!({"type": "text", "text": "new"}))
                .expect("content parses"),
        };
        assert!(!mapper.map(ClineEvent::Update(live)).is_empty());
    }

    /// No turn may claim a context reading, since nothing on this wire reports
    /// one — a ring drawn off an invented figure is worse than one drawn empty.
    #[test]
    fn a_finished_turn_claims_no_usage() {
        let usage = replay(LIVE_TURN).into_iter().find_map(|e| match e.payload {
            P::TurnCompleted { usage, .. } => Some(usage),
            _ => None,
        });

        assert_eq!(usage, Some(None), "the turn closes, and reports no tokens");
    }

    /// The name is cut at the colon, and only where the head is an identifier:
    /// a sentence with a colon in it is prose, not a tool.
    #[test]
    fn a_title_yields_its_tool_name_or_the_kind() {
        assert_eq!(tool_name(Some("run_commands: ls -la"), ToolKind::Execute), "run_commands");
        assert_eq!(tool_name(Some("editor"), ToolKind::Edit), "editor");
        assert_eq!(tool_name(Some("Reading the file: a.txt"), ToolKind::Read), "read");
        assert_eq!(tool_name(None, ToolKind::Execute), "shell");
        assert_eq!(tool_name(Some("   "), ToolKind::Fetch), "fetch");
    }

    /// Every kind ACP publishes has to land somewhere: a tool typed `Other`
    /// draws as a plain row, but a *read* typed that way loses its file.
    #[test]
    fn acp_kinds_map_onto_drays_own() {
        assert_eq!(tool_type(ToolKind::Execute), ToolType::Shell);
        assert_eq!(tool_type(ToolKind::Read), ToolType::FileRead);
        assert_eq!(tool_type(ToolKind::Edit), ToolType::FileEdit);
        assert_eq!(tool_type(ToolKind::Search), ToolType::Search);
        assert_eq!(tool_type(ToolKind::Fetch), ToolType::Web);
        assert_eq!(tool_type(ToolKind::Other), ToolType::Other);
    }
}
