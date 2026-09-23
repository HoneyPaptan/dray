//! Cline, spoken over `cline --acp`.
//!
//! One child per session, ACP over stdio — the framing is
//! [`codex::rpc`](crate::harness::codex::rpc) as-is and the vocabulary is
//! [`opencode`](crate::harness::opencode)'s, this being the fourth harness here
//! of that shape. The structural fact they all share: **a prompt is a request
//! that blocks for the whole turn.** `session/prompt` answers with the stop
//! reason once the model is done, so a turn ends as a *response* rather than a
//! notification, and the read loop watches that id itself.
//!
//! What differs, all of it measured against 3.0.64 rather than inherited:
//!
//! - **A promptless `session/new` leaves no history.** Three of them left
//!   `cline history` saying "No history found", where opencode files one per
//!   ACP session. That is what lets [`models`] probe over ACP and read display
//!   names no CLI subcommand publishes.
//! - **`session/new` answers `modes` and `models` beside `configOptions`**, so
//!   opening a session *is* reading the model list.
//! - **Stance has a real per-session surface**: `mode` is `plan` or `act` and
//!   `auto_approve` is a boolean beside it, so `manual` is honoured here where
//!   opencode can only defer to the reader's own config.
//! - **`session/set_config_option` wants the option's `type` echoed back.**
//!   Omitting it works for a select and is refused for a boolean, with a
//!   message naming the type and nothing about the option.
//! - **There is no usage of any kind on the wire**, so the composer's context
//!   ring is drawn empty rather than off an invented figure. Spend is a
//!   different question and is answered: Cline writes each request's cost into
//!   its own session file, which [`usage::spent`] sums onto the finished turn.
//!   The ring still has nothing, since no window size is published anywhere.
//! - **A resume replays the whole conversation** as ordinary updates, which
//!   Dray's log already holds — see [`mapper::Mapper::begin_replay`].

pub mod mapper;
pub mod models;
pub mod parser;
pub mod permissions;
pub mod usage;

use crate::events::{AgentEvent, AgentEventPayload, ApprovalPolicy};
use crate::harness::claude_code::permissions::PendingPermissions;
use crate::harness::codex::rpc::{Incoming, RpcClient};
use crate::harness::{read_stderr, record_failure, Harness::Cline};
use crate::models::Model;
use crate::session::{QueuedMessages, Session, StatusTracker, Transport};
use crate::store::{self, next_seq_by_session_id};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStdout, Command},
    sync::Mutex,
    time::Duration,
};

/// ACP protocol version Cline speaks — its handshake answers `1`.
pub(crate) const PROTOCOL_VERSION: u64 = 1;

/// Dray's rules, which Cline has nowhere to put but a prompt.
///
/// `cline --acp` takes no flag carrying instructions and `session/new` has no
/// field for them. The reader's own `.clinerules` is Cline's surface for this
/// and is theirs, not ours to write into. So the text rides the first prompt of
/// a new session and nothing else, since a resumed session has the rules in its
/// own history.
///
/// opencode's file, shared: the two say the same things about the same tools.
const SYSTEM_PROMPT: &str = include_str!("../opencode/system_prompt.md");

/// The tag the rules are wrapped in, so anything reading the prompt back can
/// find where they stop.
const PREAMBLE_TAG: &str = "dray_system_prompt";

/// How long a child is given to leave after EOF before it is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// The connection every write is addressed to.
///
/// Cloneable, and the read loop holds a clone: both halves have to agree about
/// which prompt is running, since the reader is what sees it answered.
#[derive(Clone, Debug)]
pub struct ClineSession {
    pub client: RpcClient,
    /// Cline's own session id, which is also the resume handle. `_meta.sessionId`
    /// on `session/new` is ignored, measured — so the index carries a
    /// `thread_id` the way opencode's and fx's do.
    pub id: String,
    /// The in-flight `session/prompt`, or `None` between turns.
    pub prompt_id: Arc<std::sync::Mutex<Option<i64>>>,
    /// Whether the next prompt still owes Dray's rules.
    preamble: Arc<AtomicBool>,
}

/// The model the session actually opened on, as Cline reported it.
///
/// Read from the `configOptions` every open and every config write answers
/// with, so what the index records is what is running rather than what was
/// asked for.
pub fn landed_model(config: &parser::ConfigOptions) -> Option<crate::models::ModelId> {
    config.current("model").map(crate::models::ModelId::new)
}

/// The tool a permission request is about, named without its arguments.
///
/// Cline's `title` there is `run_commands: pwd && ls -la` — the tool, a colon,
/// then the whole command — so the card would otherwise be headed by a line the
/// row under it already draws.
pub(crate) fn tool_name_of(request: &parser::PermissionRequest) -> String {
    mapper::tool_name_for_card(request.tool_call.title.as_deref(), request.tool_call.kind)
}

/// Starts a child and opens its session.
#[allow(clippy::too_many_arguments)]
pub async fn init(
    session_id: &str,
    model: Option<&Model>,
    permission_mode: ApprovalPolicy,
    cwd: &str,
    session_cwd: &str,
    is_new_session: bool,
    app: &AppHandle,
) -> Result<Session> {
    // Ahead of the spawn: everything between the spawn and the kill-wrapped
    // `open_session` below has to be infallible, or a `?` returns leaving a
    // child nothing can reach.
    let seq_start = if is_new_session {
        0
    } else {
        next_seq_by_session_id(session_id).await?
    };

    let bin = crate::binpath::cline().await;
    let mut command = Command::new(&bin);

    if let Some(endpoint) = crate::orchestration::child_endpoint() {
        command.env("DRAY_ENDPOINT", endpoint);
    }

    // `--cwd` as well as the process directory: Cline resolves its own config
    // and its `.clinerules` against the directory it is told about, and the
    // session's directory is not always the one the child is spawned in — a
    // worktree session spawns at the project root.
    command.args(["--acp", "--cwd", session_cwd]);

    let mut child = command
        .current_dir(cwd)
        .env("DRAY_SESSION_ID", session_id)
        .env("PATH", crate::harness::agent_path(&bin))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't start cline")?;

    let stdin = child.stdin.take().context("failed to take stdin")?;
    let stdout = child.stdout.take().context("failed to take stdout")?;
    let stderr = child.stderr.take().context("failed to take stderr")?;

    let client = RpcClient::new(stdin);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let pending: PendingPermissions = Default::default();

    let reader = ReaderHandles {
        client: client.clone(),
        session_id: session_id.to_string(),
        session_cwd: session_cwd.to_string(),
        pending: pending.clone(),
        app: app.clone(),
    };

    let seq = Arc::new(AtomicU64::new(seq_start));
    let events: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let status: Arc<Mutex<StatusTracker>> = Arc::new(Mutex::new(StatusTracker::default()));
    let queued: QueuedMessages = Arc::new(Mutex::new(Vec::new()));
    // Armed for a resume before the read loop starts, since the replay begins
    // arriving the moment `session/load` is written.
    let replaying = Arc::new(AtomicBool::new(!is_new_session));

    tokio::spawn({
        let events = events.clone();
        let status = status.clone();
        let queued = queued.clone();
        let seq = seq.clone();
        let replaying = replaying.clone();
        async move {
            if let Err(error) =
                read_stdout(stdout, reader, ready_rx, events, status, queued, seq, replaying).await
            {
                eprintln!("Failed to read cline stdout: {error}");
            }
        }
    });

    tokio::spawn(async move {
        if let Err(error) = read_stderr(Cline, stderr).await {
            eprintln!("Failed to read cline stderr: {error}");
        }
    });

    let (cline_id, config) =
        match open_session(&client, session_id, session_cwd, is_new_session).await {
            Ok(opened) => opened,
            Err(error) => {
                // Post-spawn, so the child is running with nobody left to talk
                // to it. A `Child` is not reaped on drop.
                let _ = child.kill().await;
                return Err(error);
            }
        };

    let session = ClineSession {
        client,
        id: cline_id,
        prompt_id: Arc::new(std::sync::Mutex::new(None)),
        preamble: Arc::new(AtomicBool::new(owes_preamble(is_new_session, seq_start))),
    };

    // The model on **both** paths: there is no `--model` to ride the spawn, so
    // a creation that skipped this would run on whatever Cline's own config
    // names and silently disagree with the picker that sent it.
    //
    // **Not fatal.** A model the signed-in provider no longer serves would
    // otherwise make its own session unopenable, so the refusal is reported in
    // the transcript and the session runs on Cline's default, with `landed`
    // below recording what that actually is.
    let mut config = config;
    if let Some(model) = model {
        match set_model(&session, model).await {
            Ok(answer) => config = answer,
            Err(error) => {
                let refusal = format!(
                    "Cline would not run {}: {error}. This session is running on its own default model instead.",
                    model.arg
                );
                crate::session::report_session_error(
                    session_id, Cline, &refusal, &seq, &events, app,
                )
                .await;
            }
        }
    }

    // A stance that will not apply is the fatal one: the session would run
    // freer or narrower than the reader asked, which is not something to report
    // and carry on from.
    if let Err(error) = set_stance(&session, permission_mode).await {
        let _ = child.kill().await;
        return Err(error);
    }

    let landed = landed_model(&config);
    let _ = ready_tx.send((session.clone(), replaying));

    Ok(Session {
        id: session_id.to_string(),
        child,
        stdin: Transport::Cline(session),
        harness: Cline,
        // What Cline says it is on, not what was asked for.
        model: landed.unwrap_or_default(),
        // No level exists to record: ACP publishes none and takes none.
        effort: None,
        permission_mode,
        // No fast mode either — nothing in its config, its flags or its session
        // options names one.
        fast: false,
        events,
        seq,
        status,
        pending_permissions: pending,
        queued,
    })
}

/// `initialize`, then `session/new` or `session/load`. Answers Cline's own id
/// and the options the session opened with.
async fn open_session(
    client: &RpcClient,
    session_id: &str,
    session_cwd: &str,
    is_new_session: bool,
) -> Result<(String, parser::ConfigOptions)> {
    client
        .request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                // No `fs` and no `terminal`: Cline reads and writes through its
                // own tools, and advertising either would invite requests this
                // build cannot serve.
                "clientCapabilities": {},
                "clientInfo": {"name": "dray", "title": "Dray", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;

    // Empty, and deliberately so: Cline merges the reader's own MCP config
    // itself — `cline mcp` is its surface for that — so a list sent here would
    // be a second source for servers it already knows about.
    let mcp_servers = json!([]);

    if is_new_session {
        let answer = client
            .request(
                "session/new",
                json!({"cwd": session_cwd, "mcpServers": mcp_servers}),
            )
            .await?;
        let id = answer
            .get("sessionId")
            .and_then(Value::as_str)
            .context("session/new answered with no session id")?
            .to_string();
        // Before the first prompt, so a child dying mid-turn still leaves a
        // session to resume rather than one that silently starts over.
        store::set_session_thread_id(session_id, &id).await?;
        return Ok((id, parser::ConfigOptions::of(&answer)));
    }

    let recorded = store::get_session_index_item(session_id)
        .await?
        .and_then(|item| item.thread_id)
        .context("this session has no cline session to resume")?;

    // `session/load`, advertised as `loadSession` on the handshake and verified
    // across processes: a fresh child loaded this id and replayed the turn a
    // previous one had run.
    let answer = client
        .request(
            "session/load",
            json!({"sessionId": recorded, "cwd": session_cwd, "mcpServers": mcp_servers}),
        )
        .await?;

    Ok((recorded, parser::ConfigOptions::of(&answer)))
}

/// Moves a live session onto a model, answering the options it reports back.
pub async fn set_model(session: &ClineSession, model: &Model) -> Result<parser::ConfigOptions> {
    set_config(session, "model", "select", json!(model.arg)).await
}

/// Which of Cline's two session modes a stance lands on.
///
/// `plan` and `act` are the whole list, measured. Plan explores without
/// touching files; everything else acts.
pub fn mode_for(mode: ApprovalPolicy) -> &'static str {
    match mode {
        ApprovalPolicy::Plan => "plan",
        _ => "act",
    }
}

/// Whether a stance means "do not ask".
///
/// Cline's `auto_approve` is a real per-session boolean, which is what lets
/// `manual` be honoured here rather than deferred to the reader's own config
/// the way opencode's is. `bypassPermissions` lands on the same value as `auto`
/// — there is no third, wider setting to reach — so the two are identical in
/// effect and only `auto` is claimed as honoured.
pub fn auto_approves(mode: ApprovalPolicy) -> bool {
    !matches!(mode, ApprovalPolicy::Manual)
}

/// Moves a live session's stance: the mode, then whether it asks.
///
/// Both writes, and both fatal. A stance half-applied is the case worth
/// refusing over: a session that took `plan` and not `auto_approve: false` runs
/// wider than the reader asked, and one that took neither is running a stance
/// nobody picked.
pub async fn set_stance(session: &ClineSession, mode: ApprovalPolicy) -> Result<()> {
    set_config(session, "mode", "select", json!(mode_for(mode))).await?;
    set_config(
        session,
        "auto_approve",
        "boolean",
        json!(auto_approves(mode)),
    )
    .await?;
    Ok(())
}

/// One `session/set_config_option`.
///
/// Two field names worth pinning. The id is **`configId`**, which the ACP draft
/// calls `optionId` — Cline answers that with `expected string, received
/// undefined`. And the option's own **`type`** has to be echoed: omitted, a
/// select still lands and a boolean is refused with `Invalid input: expected
/// "boolean"`, a message that names the type and nothing about the option being
/// set.
async fn set_config(
    session: &ClineSession,
    config_id: &str,
    kind: &str,
    value: Value,
) -> Result<parser::ConfigOptions> {
    let answer = session
        .client
        .request(
            "session/set_config_option",
            json!({
                "sessionId": session.id,
                "configId": config_id,
                "type": kind,
                "value": value,
            }),
        )
        .await?;

    Ok(parser::ConfigOptions::of(&answer))
}

/// Whether this session still owes Dray's rules.
///
/// A resumed session that has logged nothing is one whose first prompt never
/// landed, so it owes them the way a new one does.
fn owes_preamble(is_new_session: bool, seq_start: u64) -> bool {
    is_new_session || seq_start == 0
}

/// The reader's text with [`SYSTEM_PROMPT`] behind it, wrapped in
/// [`PREAMBLE_TAG`] so where the rules start and stop is mechanical.
///
/// **Behind, not in front**, the reading fx measured and opencode kept: an
/// agent that derives a session title from the first turn titles it after 4KB
/// of house rules rather than after the work.
fn with_preamble(text: &str) -> String {
    format!("{text}{}", preamble_block())
}

/// Exactly what [`with_preamble`] appends, as one string, so [`crate::title`]
/// can cut back off what this puts on by equality rather than by pattern.
pub(crate) fn preamble_block() -> String {
    format!("\n\n<{PREAMBLE_TAG}>\n{SYSTEM_PROMPT}\n</{PREAMBLE_TAG}>")
}

/// Writes one prompt as a turn.
///
/// Sent with an id and **no waiter**: the response is the turn's end, minutes
/// away, and the read loop settles it on the id kept here.
///
/// Only the *transport* text carries the rules: [`crate::session`] logs
/// `user_message` from the reader's own string, so the transcript never sees
/// the block and there is nothing to strip on the way out.
pub async fn start_turn(session: &ClineSession, text: &str) -> Result<()> {
    // Read, not taken: a send that fails hands the line to nobody, and the
    // reader's retry is then a turn that never learns the rules.
    let owed = session.preamble.load(std::sync::atomic::Ordering::Relaxed);
    let sent = if owed {
        with_preamble(text)
    } else {
        text.to_string()
    };

    // Held across the send, which only hands the line to the writer task: an
    // outright refusal can answer before this returns, and `prompt_answer`
    // takes this same lock, so it cannot read the id before it is written.
    let mut running = session.prompt_id.lock().expect("cline prompt id poisoned");
    let id = session.client.request_detached(
        "session/prompt",
        json!({
            "sessionId": session.id,
            "prompt": [{"type": "text", "text": sent}],
        }),
    )?;
    *running = Some(id);
    session
        .preamble
        .store(false, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Stops the running turn. A notification: nothing is acknowledged, the running
/// tool is killed, and the prompt answers `cancelled` — which is what closes
/// the turn, so the stop is reported through the same path a finished one is.
pub fn cancel(session: &ClineSession) -> Result<()> {
    session
        .client
        .notify("session/cancel", json!({"sessionId": session.id}))
}

/// Ends the child cleanly: EOF, then a kill if it lingers.
///
/// No `session/close`: Cline's handshake advertises `loadSession` and the
/// prompt capabilities and nothing else, so asking for one would be a method it
/// never offered.
pub async fn shutdown(child: &mut Child, session: &ClineSession) {
    session.client.close();

    if tokio::time::timeout(SHUTDOWN_GRACE, child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

/// The handles the read loop needs that are not per-event state.
struct ReaderHandles {
    client: RpcClient,
    session_id: String,
    session_cwd: String,
    pending: PendingPermissions,
    app: AppHandle,
}

#[allow(clippy::too_many_arguments)]
async fn read_stdout(
    stdout: ChildStdout,
    handles: ReaderHandles,
    ready: tokio::sync::oneshot::Receiver<(ClineSession, Arc<AtomicBool>)>,
    events: Arc<Mutex<Vec<AgentEvent>>>,
    status: Arc<Mutex<StatusTracker>>,
    queued: QueuedMessages,
    seq: Arc<AtomicU64>,
    replaying: Arc<AtomicBool>,
) -> Result<()> {
    let mut lines = BufReader::new(stdout).lines();
    let mut mapper = mapper::Mapper::new(handles.session_id.clone(), seq.clone());
    if replaying.load(std::sync::atomic::Ordering::Relaxed) {
        mapper.begin_replay();
    }

    // Held until the session exists. Lines before it are routed — the
    // handshake's answers have to reach their waiters — but mapped to nothing.
    let mut ready = Some(ready);
    let mut transport: Option<Transport> = None;
    // Cline's own session id, kept beside the transport because the spend it
    // addresses on disk outlives any one event.
    let mut thread_id: Option<String> = None;

    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            // stdout closed (the child exited) or a read error — either way no
            // more of this turn is coming. Break to the cleanup below.
            Ok(None) => break,
            Err(err) => {
                eprintln!("[cline stdout err] {err}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        if transport.is_none() {
            if let Some(rx) = &mut ready {
                match rx.try_recv() {
                    Ok((session, _)) => {
                        thread_id = Some(session.id.clone());
                        transport = Some(Transport::Cline(session));
                        ready = None;
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => ready = None,
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                }
            }
        }

        // A prompt going out ends the replay window: from there every update is
        // this session's own work rather than its history.
        if let Some(Transport::Cline(session)) = transport.as_ref() {
            if session
                .prompt_id
                .lock()
                .expect("cline prompt id poisoned")
                .is_some()
            {
                mapper.end_replay();
            }
        }

        // The prompt's own answer, read ahead of the demux: nothing waits on
        // it, so `accept` would file it as stray.
        let mut event: Option<parser::ClineEvent> = None;
        if let Some(Transport::Cline(session)) = transport.as_ref() {
            if let Some(done) = prompt_answer(session, &line) {
                event = Some(done);
            }
        }

        let event = match event {
            Some(event) => event,
            None => match handles.client.accept(&line).await {
                Incoming::Notification { method, params } => {
                    match parser::parse_notification(&method, params) {
                        Ok(parser::ClineEvent::Unknown) => {
                            record_failure(Cline, &handles.session_id, "unknown_method", &method, &line)
                                .await;
                            continue;
                        }
                        Ok(event) => event,
                        Err(err) => {
                            record_failure(Cline, &handles.session_id, "map", &err.to_string(), &line)
                                .await;
                            continue;
                        }
                    }
                }

                // Every server request blocks the turn until it is answered, so
                // silence stalls the session exactly as an unanswered
                // `can_use_tool` does.
                Incoming::Request { id, method, params } => {
                    if method == "session/request_permission" {
                        if let Err(err) = raise_permission(&handles, &mut mapper, id, params).await {
                            record_failure(Cline, &handles.session_id, "unsupported_request", &err.to_string(), &line)
                                .await;
                            let _ = handles
                                .client
                                .respond(id, json!({"outcome": {"outcome": "cancelled"}}));
                        }
                    } else {
                        // `fs/*` and `terminal/*` were not advertised, so one
                        // arriving is Cline asking past the capabilities it was
                        // given. A protocol error leaves it to decide.
                        record_failure(Cline, &handles.session_id, "unsupported_request", &method, &line)
                            .await;
                        let _ = handles.client.respond_err(
                            id,
                            -32601,
                            "This client cannot answer that request yet.",
                        );
                    }
                    continue;
                }

                Incoming::Response { id, matched: false } => {
                    let detail = format!("no caller waiting on id {id}");
                    record_failure(Cline, &handles.session_id, "stray_response", &detail, &line).await;
                    continue;
                }
                Incoming::Response { .. } => continue,

                Incoming::Malformed => {
                    record_failure(Cline, &handles.session_id, "parse", "not a JSON-RPC message", &line)
                        .await;
                    continue;
                }
            },
        };

        let Some(transport) = transport.as_ref() else {
            continue;
        };

        let ingest = crate::session::Ingest {
            session_id: &handles.session_id,
            harness: Cline,
            session_cwd: &handles.session_cwd,
            events: &events,
            status: &status,
            queued: &queued,
            flush_seq: &seq,
            flush_events: &events,
            flush_transport: transport,
        };

        for mut agent_event in mapper.map(event) {
            attach_spend(&mut agent_event, thread_id.as_deref());
            crate::session::ingest(&ingest, agent_event, &handles.app).await;
        }
    }

    // The child's stdout has ended. If a prompt was still in flight its answer
    // will never arrive, so close the turn as a failure — without which the
    // session hangs `in_progress` forever, its queue with no boundary to drain
    // at. The queue is stranded *first* (reported and cleared), so the closing
    // turn's boundary flush finds nothing to hand the dead child.
    if let Some(transport @ Transport::Cline(session)) = transport.as_ref() {
        // Clear it either way — the child is gone, so a Stop pressed now names
        // nothing — and remember whether a turn was open to close it below.
        let outstanding = session
            .prompt_id
            .lock()
            .expect("cline prompt id poisoned")
            .take()
            .is_some();

        crate::session::strand_queue_on_exit(
            &handles.session_id,
            Cline,
            &queued,
            &seq,
            &events,
            &handles.app,
        )
        .await;

        if outstanding {
            let ingest = crate::session::Ingest {
                session_id: &handles.session_id,
                harness: Cline,
                session_cwd: &handles.session_cwd,
                events: &events,
                status: &status,
                queued: &queued,
                flush_seq: &seq,
                flush_events: &events,
                flush_transport: transport,
            };
            for mut agent_event in mapper.map(parser::ClineEvent::PromptFailed {
                message: "cline exited before finishing this turn.".to_string(),
            }) {
                attach_spend(&mut agent_event, thread_id.as_deref());
                crate::session::ingest(&ingest, agent_event, &handles.app).await;
            }
        }
    }

    Ok(())
}

/// Fills a finished turn's spend from Cline's own session file.
///
/// The mapper cannot: nothing on the wire carries a figure, and the file is
/// Cline's rather than ours. Same seam `head` already uses — a field the
/// mapper leaves `None` and the layer that can answer it fills in. A turn that
/// ended in failure is filled too, since the requests it made were still paid
/// for.
fn attach_spend(event: &mut AgentEvent, thread_id: Option<&str>) {
    let AgentEventPayload::TurnCompleted { usage, .. } = &mut event.payload else {
        return;
    };
    if let Some(id) = thread_id {
        *usage = usage::spent(id);
    }
}

/// The running prompt's response, where `line` is it.
///
/// A response with no method and the prompt's id. Both a result and an error
/// close the turn, and the id is cleared here so a Stop pressed after the
/// answer names nothing.
fn prompt_answer(session: &ClineSession, line: &str) -> Option<parser::ClineEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("method").is_some() {
        return None;
    }
    let id = value.get("id").and_then(Value::as_i64)?;

    let mut running = session.prompt_id.lock().expect("cline prompt id poisoned");
    if *running != Some(id) {
        return None;
    }
    *running = None;

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("cline could not run this prompt")
            .to_string();
        return Some(parser::ClineEvent::PromptFailed { message });
    }

    let response: parser::PromptResponse = value
        .get("result")
        .cloned()
        .and_then(|r| serde_json::from_value(r).ok())
        .unwrap_or_default();
    Some(parser::ClineEvent::PromptDone(response))
}

/// Turns one permission request into the card that answers it.
///
/// Registered before it is emitted, so a button pressed the instant the card
/// draws finds the entry waiting. The reply goes out from
/// [`Session::respond_permission`](crate::session::Session::respond_permission)
/// when the user picks; until then Cline is blocked.
async fn raise_permission(
    handles: &ReaderHandles,
    mapper: &mut mapper::Mapper,
    rpc_id: i64,
    params: Value,
) -> Result<()> {
    let request: parser::PermissionRequest = serde_json::from_value(params)?;
    let (pending, options) = permissions::pending_for(&request, rpc_id);

    let request_id = rpc_id.to_string();
    handles
        .pending
        .lock()
        .expect("pending permissions mutex poisoned")
        .insert(request_id.clone(), pending);

    let input = request.tool_call.raw_input.clone().unwrap_or(json!({}));
    // Cline's shell takes a `commands` array where every other harness here
    // takes one string, so the card reads both.
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            input
                .get("commands")
                .and_then(Value::as_array)
                .map(|all| {
                    all.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" && ")
                })
                .filter(|joined| !joined.is_empty())
        });
    let path = input.get("path").and_then(Value::as_str).map(str::to_string);

    let event = mapper.synthesize(AgentEventPayload::PermissionRequested {
        request_id,
        tool_use_id: request.tool_call.tool_call_id.clone(),
        tool_name: tool_name_of(&request),
        display_name: None,
        // The command or the path, which is what the card draws as the subject.
        title: command.clone().or_else(|| path.clone()),
        description: None,
        input,
        blocked_path: path,
        decision_reason: None,
        decision_reason_type: None,
        agent_id: None,
        options,
    });

    // Emitted, never logged — only the child that asked can answer.
    handles.app.emit("agent_event", &event)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan is the only stance that stops Cline touching files, and `manual` is
    /// the only one that leaves the asking on — which is what makes it
    /// genuinely honoured here rather than deferred to the reader's config.
    #[test]
    fn a_stance_is_a_mode_and_whether_it_asks() {
        assert_eq!(mode_for(ApprovalPolicy::Plan), "plan");
        assert_eq!(mode_for(ApprovalPolicy::Manual), "act");
        assert_eq!(mode_for(ApprovalPolicy::Auto), "act");
        assert_eq!(mode_for(ApprovalPolicy::BypassPermissions), "act");

        assert!(!auto_approves(ApprovalPolicy::Manual));
        assert!(auto_approves(ApprovalPolicy::Auto));
        assert!(auto_approves(ApprovalPolicy::BypassPermissions));
        assert!(auto_approves(ApprovalPolicy::Plan));
    }

    /// A new session owes the rules, and so does a resumed one that never
    /// logged a turn — its first prompt never landed.
    #[test]
    fn the_rules_ride_a_first_prompt_only() {
        assert!(owes_preamble(true, 0));
        assert!(owes_preamble(false, 0));
        assert!(!owes_preamble(false, 12));
    }
}
