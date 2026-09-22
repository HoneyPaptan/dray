//! fx's ACP vocabulary onto Dray's.
//!
//! Three things are synthesized rather than read, and each is noted where it
//! is minted. fx sends no turn-started line — the turn opens when the prompt
//! request is written and closes when it answers — so `TurnStarted` is minted
//! on the first update after a prompt. It sends no "requesting" ping, so
//! `ModelRequestStarted` is minted with it and after every tool result. And a
//! thought chunk carries no id, so one thinking block runs until the next
//! non-thought update.

use crate::events::{
    usage::ContextWindow, AgentEvent, AgentEventPayload, BlockRef, BlockType, DeltaEvent,
    SessionInfo, ToolResult, ToolType, TurnStatus, Usage,
};
use crate::harness::{mentions_any, Harness};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;

use super::parser::{
    OpencodeEvent, PromptResponse, RawOutput, SessionUpdate, ToolContent, ToolKind, ToolStatus,
};

/// What opencode says when a provider wants a login. Written to
/// under-match: a wording missed costs the login button and keeps the
/// sentence. Read off the live refusal `fx needs a Grok subscription login for
/// this model. Run fx login grok.`
const LOGIN_NEEDLES: &[&str] = &["login", "log in", "sign in", "not authenticated"];

/// fx's own diagnostics, which it emits into the agent message stream rather
/// than to stderr: context-limit truncation and skill-discovery warnings, each
/// a chunk of its own before the answer. Matched on their fixed machine
/// prefixes — a real reply opens with neither.
///
// ponytail: prefix match on the two observed shapes; a new diagnostic prefix fx
// adds later shows through until it is listed here.
fn is_fx_diagnostic(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("[context]") || t.starts_with("skill discovery warning:")
}

/// A streamed block still open, and the text it has accumulated so far — the
/// committed event supersedes the deltas, so the whole text is kept.
struct OpenBlock {
    id: String,
    kind: BlockType,
    text: String,
}

/// Per-session state the mapping needs across lines.
pub struct Mapper {
    /// Dray's own id, never fx's. Every event the frontend routes is keyed on
    /// this, and the two are only joined on the index entry.
    session_id: String,
    seq: Arc<AtomicU64>,
    /// Whether a prompt is running. Read by the read loop too: a title landing
    /// outside a turn is `session/resume` restating one Dray already holds.
    turn_open: bool,
    /// The one block streaming right now. fx interleaves thought and text
    /// chunks with no ids on the thoughts, so at most one block is open and a
    /// chunk of the other kind closes it.
    open: Option<OpenBlock>,
    /// Ids handed to thought blocks, which carry none of their own.
    thoughts: u64,
    /// The newest occupancy reading, folded onto the turn's own
    /// `TurnCompleted` — the composer's ring reads it back out of the log, and
    /// `UsageUpdate` is not persisted.
    occupancy: Option<ContextWindow>,
    /// Text a running call has streamed, by call id. A shell's stdout arrives
    /// one `in_progress` update per line and its closing update carries only
    /// fx's replay blob, so the result is what accumulated here.
    outputs: HashMap<String, String>,
    /// Message ids whose chunks are fx diagnostics, not the answer — kept so a
    /// diagnostic streamed over several chunks is dropped whole, not only its
    /// first fragment.
    suppressed: HashSet<String>,
    /// Calls opened but not yet announced, by id. See [`Self::update`]: the
    /// opening `tool_call` carries no arguments worth drawing, so the row is
    /// held until an update brings them.
    opening: HashMap<String, Opening>,
    /// What the session has cost so far, as opencode prices it. Kept because
    /// `usage_update` is never persisted and the picker reads spend back out of
    /// the log — so the figure has to ride the turn's own `TurnCompleted`.
    cost_usd: Option<f64>,
}

/// A call announced by `tool_call` and not yet drawn.
struct Opening {
    /// The opening update's `title`, which on this wire **is** the tool's name
    /// — `bash`, `read`. Later updates restate `title` as the work itself
    /// (`echo hi > /tmp/oc/a.txt`), so only the first one may name the tool.
    name: String,
    kind: ToolKind,
    input: Option<Value>,
}

impl Mapper {
    pub fn new(session_id: String, seq: Arc<AtomicU64>) -> Self {
        Self {
            session_id,
            seq,
            turn_open: false,
            open: None,
            thoughts: 0,
            occupancy: None,
            outputs: HashMap::new(),
            suppressed: HashSet::new(),
            opening: HashMap::new(),
            cost_usd: None,
        }
    }

    pub fn map(&mut self, event: OpencodeEvent) -> Vec<AgentEvent> {
        match event {
            OpencodeEvent::Update(update) => self.update(update),
            OpencodeEvent::PromptDone(response) => self.prompt_done(response),
            OpencodeEvent::PromptFailed { message } => self.prompt_failed(message),
            OpencodeEvent::Unknown => Vec::new(),
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
                // fx writes its own startup diagnostics — context-limit
                // truncation, skill-discovery warnings — into the message
                // stream as chunks of their own ahead of the answer. Drop them,
                // remembering the id so a diagnostic split across chunks goes
                // whole rather than leaving its tail on screen.
                if self.suppressed.contains(&id) || is_fx_diagnostic(text) {
                    self.suppressed.insert(id);
                    return Vec::new();
                }
                let mut out = self.ensure_turn();
                out.extend(self.stream(id, BlockType::Text, text));
                out
            }

            SessionUpdate::AgentThoughtChunk { content } => {
                let Some(text) = content.text() else {
                    return Vec::new();
                };
                let mut out = self.ensure_turn();
                // Thoughts carry no id, so one block runs until something else
                // arrives. A thought after a text block is a new block; a
                // thought after a thought continues it.
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
                name,
                title,
                kind,
                raw_input,
                ..
            } => {
                // Held, not drawn. opencode opens a call with the tool's name
                // and almost nothing else — a `bash` call arrives carrying
                // `{"cwd": "…"}` and the command lands on the *next* update —
                // so a row drawn from this one says "bash" and never says what
                // ran. The row is minted below instead, off the first update
                // that brings arguments or off the close, whichever comes
                // first. Cost, stated: a call whose updates never carry
                // arguments is drawn only when it finishes.
                self.opening.insert(
                    tool_call_id,
                    Opening {
                        // `name` is fx's field and opencode sends none, so the
                        // opening title stands in — it is the tool's name here.
                        name: name
                            .or(title)
                            .unwrap_or_else(|| kind_name(kind).to_string()),
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

                // The update's own arguments replace the opening call's,
                // which is where the command actually arrives.
                if let Some(held) = self.opening.get_mut(&tool_call_id) {
                    if let Some(kind) = kind {
                        held.kind = kind;
                    }
                    if let Some(input) = raw_input {
                        if !is_thin(&input) {
                            held.input = Some(input);
                        }
                    }
                }

                let final_status = status.filter(|s| s.is_final());
                // Arguments, or the last chance to draw the row at all.
                let mut out = self.announce(&tool_call_id, &raw_output, final_status.is_some());

                let Some(status) = final_status else {
                    // Streamed output. Kept for the result rather than drawn —
                    // the row draws the committed result, and a shell's stdout
                    // is what these carry.
                    if !text.is_empty() {
                        self.outputs
                            .entry(tool_call_id)
                            .or_default()
                            .push_str(&text);
                    }
                    let _ = (title, kind);
                    return out;
                };

                let streamed = self.outputs.remove(&tool_call_id).unwrap_or_default();
                let result = ToolResult {
                    text: result_text(streamed, text),
                    is_error: status == ToolStatus::Failed,
                    structured: None,
                    // `metadata.exit`, which is opencode's own spelling and the
                    // only place a shell's status is reported.
                    exit_code: raw_output
                        .as_ref()
                        .and_then(|o| o.metadata.exit)
                        .map(|code| code as i32),
                    // Nothing on this wire times a call.
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

            SessionUpdate::UsageUpdate { used, size, cost } => {
                let window = match (used, size) {
                    (Some(used), Some(size)) if size > 0 => Some(ContextWindow {
                        used_tokens: used,
                        max_tokens: size,
                    }),
                    _ => None,
                };
                if window.is_some() {
                    self.occupancy = window;
                }
                // A running total, so the newest reading is the whole answer.
                if let Some(cost) = cost {
                    self.cost_usd = Some(cost.amount);
                }
                vec![self.event(AgentEventPayload::UsageUpdate(Usage {
                    context_window: window,
                    cost_usd: self.cost_usd,
                    ..Default::default()
                }))]
            }

            // Read by the read loop off the parsed update, not mapped: a title
            // is a fact about the index row, not a transcript event.
            SessionUpdate::SessionInfoUpdate { .. } => Vec::new(),

            // Recorded by the read loop off the parsed update, not mapped: a
            // command list is a fact about the picker, not a transcript event.
            SessionUpdate::AvailableCommandsUpdate { .. } => Vec::new(),

            SessionUpdate::UserMessageChunk
            | SessionUpdate::CurrentModeUpdate
            | SessionUpdate::Plan
            | SessionUpdate::Unknown => Vec::new(),
        }
    }

    /// Draws a held call's row, once there is something worth drawing.
    ///
    /// Called on every update. The row is minted when the update brings
    /// arguments, or when the call closes — whichever lands first — so a call
    /// is announced exactly once and always with the best input seen. Returns
    /// empty for a call already announced, which is every update after the
    /// first.
    ///
    /// The arguments come off the update's own `rawInput` where it has one;
    /// `rawOutput` is passed only so the close can announce a call whose
    /// arguments never arrived at all.
    fn announce(
        &mut self,
        call_id: &str,
        _raw_output: &Option<RawOutput>,
        closing: bool,
    ) -> Vec<AgentEvent> {
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
            input: tool_input(held.kind, held.input),
            raw_input: None,
            // opencode's own title is the tool name on the opening update and
            // the work itself on the next, and the row already draws the work
            // off the input — so it is left unset and `toolSummary` answers.
            title: None,
        }));
        out
    }

    fn prompt_done(&mut self, response: PromptResponse) -> Vec<AgentEvent> {
        let mut out = self.close_open();

        let (status, final_text) = match response.stop_reason.as_str() {
            // `refused` answered a prompt fx declined to run at all — an image
            // on a provider that takes none, on capture. Nothing else on the
            // wire says so, so the sentence is minted here.
            "refused" | "refusal" => (
                TurnStatus::Error,
                Some("fx refused this prompt.".to_string()),
            ),
            // ACP's two other terminal reasons: the turn ended with the work
            // unfinished, and fx sends no sentence saying so.
            "max_tokens" => (
                TurnStatus::Error,
                Some("fx stopped: the model hit its output token limit.".to_string()),
            ),
            "max_turn_requests" => (
                TurnStatus::Error,
                Some("fx stopped: the turn hit its request limit.".to_string()),
            ),
            // `end_turn`, and `cancelled` — the reader's own Stop, reported as
            // a success carrying a reason nothing draws, the reading Codex's
            // `interrupted` makes.
            _ => (TurnStatus::Success, None),
        };

        let usage = response.usage;
        out.push(self.turn_completed(
            status,
            Some(response.stop_reason),
            final_text,
            false,
            Some(Usage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cached_input_tokens: usage.cache_read_tokens,
                cache_write_tokens: usage.cache_write_tokens,
                reasoning_tokens: usage.reasoning_tokens,
                context_window: self.occupancy,
                cost_usd: self.cost_usd,
                ..Default::default()
            }),
        ));
        out
    }

    /// `session/prompt` refused outright. The sentence is fx's own and usually
    /// names its cure (`Run fx login grok.`), so it is the row's text.
    fn prompt_failed(&mut self, message: String) -> Vec<AgentEvent> {
        let mut out = self.close_open();
        let auth_failed = mentions_any(&message, LOGIN_NEEDLES);
        out.push(self.turn_completed(
            TurnStatus::Error,
            None,
            Some(message),
            auth_failed,
            None,
        ));
        out
    }

    fn turn_completed(
        &mut self,
        status: TurnStatus,
        stop_reason: Option<String>,
        final_text: Option<String>,
        auth_failed: bool,
        usage: Option<Usage>,
    ) -> AgentEvent {
        let event = self.event(AgentEventPayload::TurnCompleted {
            status,
            stop_reason,
            auth_failed,
            final_text,
            usage: usage.or_else(|| {
                self.occupancy.map(|window| Usage {
                    context_window: Some(window),
                    ..Default::default()
                })
            }),
            duration_ms: None,
            // Filled by `session::ingest`, the only layer that knows the tree.
            head: None,
        });
        self.turn_open = false;
        self.outputs.clear();
        event
    }

    /// Opens the turn on its first update. fx has no turn-started line: the
    /// prompt request is the start and its answer the end, and neither passes
    /// through here — so the first thing the model says is what opens it.
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

    /// Closes the streaming block, committing its whole text: the deltas were
    /// a preview and this is what the transcript keeps.
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
            Harness::Fx,
            self.seq.fetch_add(1, Relaxed),
            // fx names no turn on the wire, and minting one here would split
            // what the reader sees as one exchange.
            None,
            None,
            payload,
        )
    }
}

/// fx has no message/block split — one message id is one block — so the index
/// is always zero.
fn block_ref(id: &str) -> BlockRef {
    BlockRef {
        message_id: id.to_string(),
        index: 0,
    }
}

/// ACP's kind onto Dray's, which is what lets a tool opencode renames tomorrow
/// still draw as what it is.
///
/// **The kind and nothing else.** fx's mapper overrides two of its own tool
/// names here, and those names are fx's — a table of opencode's would be a
/// guess about tools no capture in this tree has seen, and a wrong guess types
/// a row as something it is not. A delegated run therefore draws as an ordinary
/// tool row rather than as a subagent, which is the honest answer until one is
/// measured.
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

/// A name for a call that arrived without one — fx sends one on every capture,
/// so this is the line-survives-anything fallback.
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
/// A shell call carries fx's own dispatch fields beside the command —
/// `action`, `profile`, `yield_time_ms`, and the session's own `cwd` — which
/// gave every shell row an expanded body of machinery under one line of
/// command. The command is the whole input, and it is already the summary.
fn tool_input(kind: ToolKind, raw: Option<Value>) -> Value {
    let mut input = match raw {
        Some(Value::Object(map)) => Value::Object(map),
        Some(Value::Null) | None => json!({}),
        Some(other) => json!({ "_unparsed": other.to_string() }),
    };
    if kind == ToolKind::Execute {
        if let Some(map) = input.as_object_mut() {
            for key in ["action", "profile", "yield_time_ms", "cwd"] {
                map.remove(key);
            }
        }
    }
    input
}

/// What a finished call reports: its closing text, falling back to what it
/// streamed where the closing update carried nothing a reader wants.
///
/// The closing text wins because a streamed update is not always output. A
/// shell's is — stdout arrives line by line and its closing update carries only
/// `{"session_id":null,"state":"completed","backend":"captured",…}`, fx's own
/// bookkeeping for `fx background`, which drawn read as the command having
/// printed JSON it never printed. But `web_fetch` streams `Fetching <url>` and
/// `Converting <url>` as progress and puts the page in its closing update, so
/// preferring the stream drew the progress chatter and dropped the answer.
fn result_text(streamed: String, closing: String) -> String {
    if !closing.is_empty() {
        return closing;
    }
    streamed
}

/// Whether a call's arguments say nothing worth drawing.
///
/// opencode opens a call with a placeholder — `{}` for a read, `{"cwd": "…"}`
/// for a shell — and brings the real arguments on the next update. Both are
/// "nothing yet": a row drawn from either names a tool and no work. Judged on
/// the keys that are there rather than on a count, since `cwd` is the one field
/// the placeholder carries and a real call carries it beside the command.
fn is_thin(input: &Value) -> bool {
    match input.as_object() {
        Some(map) => map.keys().all(|key| key == "cwd"),
        // A non-object `rawInput` is not something any capture has shown; taken
        // as thin, so the row waits for something better rather than drawing a
        // bare scalar as a tool's arguments.
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::AgentEventPayload as P;

    /// One real turn against a free model: a shell call, a read, the answer,
    /// and the usage that closed it.
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
                out.extend(mapper.map(OpencodeEvent::Update(update.update)));
                continue;
            }

            if let Some(result) = value.get("result").filter(|r| r.get("stopReason").is_some()) {
                let response: PromptResponse =
                    serde_json::from_value(result.clone()).expect("prompt response parses");
                out.extend(mapper.map(OpencodeEvent::PromptDone(response)));
            }
        }

        out
    }

    /// The whole reason a call is held: opencode's opening `tool_call` carries
    /// the tool's name and a `cwd`, and the command arrives one update later.
    /// Drawn from the opening one, every shell row in every session reads
    /// "bash" with no command under it.
    #[test]
    fn a_shell_row_carries_the_command_that_ran() {
        let started: Vec<(String, Value)> = replay(LIVE_TURN)
            .into_iter()
            .filter_map(|e| match e.payload {
                P::ToolCallStarted { name, input, .. } => Some((name, input)),
                _ => None,
            })
            .collect();

        assert_eq!(started.len(), 2, "one row per call, announced exactly once");
        assert_eq!(started[0].0, "bash");
        assert_eq!(
            started[0].1.get("command").and_then(Value::as_str),
            Some("echo hi > /tmp/oc/a.txt"),
        );
        assert_eq!(started[1].0, "read");
        assert_eq!(
            started[1].1.get("filePath").and_then(Value::as_str),
            Some("/tmp/oc/a.txt"),
        );
    }

    /// `metadata.exit`, opencode's own spelling. A rename there costs no error
    /// and no failed line — only a shell row that stops saying whether the
    /// command worked — so it is pinned against the capture.
    #[test]
    fn a_finished_shell_reports_its_exit_code() {
        let codes: Vec<Option<i32>> = replay(LIVE_TURN)
            .into_iter()
            .filter_map(|e| match e.payload {
                P::ToolCallCompleted { result, .. } => Some(result.exit_code),
                _ => None,
            })
            .collect();

        assert_eq!(codes, [Some(0), None], "the read reports none, and should");
    }

    /// The cost is a running total on an event that is never persisted, so it
    /// has to ride the turn — which is what the model picker reads spend back
    /// out of.
    #[test]
    fn the_turn_carries_what_the_session_has_cost() {
        let completed = replay(LIVE_TURN)
            .into_iter()
            .find_map(|e| match e.payload {
                P::TurnCompleted { usage, .. } => usage,
                _ => None,
            })
            .expect("the capture closes its turn");

        assert_eq!(completed.cost_usd, Some(0.0));
        assert_eq!(
            completed.context_window.map(|w| w.used_tokens),
            Some(14971),
            "the occupancy is the newest reading, not a sum",
        );
    }

    /// A placeholder is anything that names no work: `{}` and a bare `cwd` are
    /// both what the opening call carries, and both have to wait.
    #[test]
    fn a_call_waits_for_arguments_worth_drawing() {
        assert!(is_thin(&serde_json::json!({})));
        assert!(is_thin(&serde_json::json!({"cwd": "/tmp/oc"})));
        assert!(!is_thin(&serde_json::json!({"command": "ls", "cwd": "/tmp"})));
        assert!(!is_thin(&serde_json::json!({"filePath": "/tmp/a.txt"})));
    }

    /// Every kind ACP publishes has to land somewhere: a tool typed `Other`
    /// draws as a plain row, which is the honest answer for a kind this build
    /// has no reading of, but a *read* typed that way loses its file.
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
