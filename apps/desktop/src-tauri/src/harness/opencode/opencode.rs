//! opencode, spoken over `opencode acp`.
//!
//! One child per session, ACP over stdio — the framing is
//! [`codex::rpc`](crate::harness::codex::rpc) as-is, and the vocabulary is the
//! one [`fx`](crate::harness::fx) already speaks, opencode being fx's upstream.
//! The structural fact both share: **a prompt is a request that blocks for the
//! whole turn.** `session/prompt` answers with the stop reason once the model
//! is done, so a turn ends as a *response* rather than a notification, and the
//! read loop watches that id itself instead of registering a waiter whose
//! timeout is meant for acknowledgements.
//!
//! What differs from fx, all of it measured against 1.18.30 rather than
//! inherited:
//!
//! - **Resume is `session/load`**, the ACP standard method, advertised as
//!   `loadSession` on the handshake. fx's `session/resume` is its own.
//! - **The id is opencode's**, not Dray's: `_meta.sessionId` on `session/new`
//!   is ignored and a `ses_…` comes back, so the index carries a `thread_id`
//!   the way fx's does rather than grok's honoured id.
//! - **There is no effort level anywhere.** A session's `configOptions` are
//!   `model` and `mode` and nothing else, so no ladder is learned, none is
//!   drawn, and none is ever sent.
//! - **The model rides no flag.** `opencode acp` takes no `--model`, so the
//!   pick is a `session/set_config_option` after the session opens — on
//!   creation as well as on resume.
//! - **MCP is opencode's own.** It merges the reader's config itself, grok's
//!   arrangement, so the server list goes over empty and there is no
//!   `mcp.rs` here.

pub mod commands;
pub mod mapper;
pub mod models;
pub mod parser;
pub mod permissions;

use crate::events::{AgentEvent, AgentEventPayload, ApprovalPolicy};
use crate::harness::claude_code::permissions::PendingPermissions;
use crate::harness::codex::rpc::{Incoming, RpcClient};
use crate::harness::{read_stderr, record_failure, Harness::Opencode};
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

/// ACP protocol version opencode speaks — its handshake answers `1`.
const PROTOCOL_VERSION: u64 = 1;

/// Dray's rules, which opencode has nowhere to put but a prompt.
///
/// `opencode acp` takes `--cwd`, `--port`, `--hostname` and logging flags and
/// nothing that carries instructions; `session/new` has no field for them. The
/// reader's own `AGENTS.md` is opencode's surface for this and is theirs, not
/// ours to write into. So the text rides the first prompt of a new session and
/// nothing else, since a resumed session has the rules in its own history.
///
/// fx's file, shared: both read `~/.claude/skills` among their global roots, so
/// the one thing a per-harness copy would say differently is already the same.
const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

/// The tag the rules are wrapped in, so anything reading the prompt back can
/// find where they stop.
const PREAMBLE_TAG: &str = "dray_system_prompt";

/// How long a child is given to leave after `session/close` and EOF before it
/// is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// The connection every write is addressed to.
///
/// Cloneable, and the read loop holds a clone: both halves have to agree about
/// which prompt is running, since the reader is what sees it answered.
#[derive(Clone, Debug)]
pub struct OpencodeSession {
    pub client: RpcClient,
    /// opencode's own session id, which is also the resume handle.
    pub id: String,
    /// The in-flight `session/prompt`, or `None` between turns.
    pub prompt_id: Arc<std::sync::Mutex<Option<i64>>>,
    /// Whether the next prompt still owes Dray's rules.
    preamble: Arc<AtomicBool>,
}

/// The model the session actually opened on, as opencode reported it.
///
/// Read from the `configOptions` every open and every config write answers
/// with, so what the index records is what is running rather than what was
/// asked for — the same lesson fx's effort recording is written from. `None`
/// where opencode named no current model, which the index reads as unset.
pub fn landed_model(config: &parser::ConfigOptions) -> Option<crate::models::ModelId> {
    config
        .current("model")
        .map(|id| crate::models::ModelId::new(id))
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

    let bin = crate::binpath::opencode().await;
    let mut command = Command::new(&bin);

    if let Some(endpoint) = crate::orchestration::child_endpoint() {
        command.env("DRAY_ENDPOINT", endpoint);
    }

    // `--cwd` as well as the process directory: opencode resolves its own
    // config, its agents and its skills against the directory it is told about,
    // and the session's directory is not always the one the child is spawned in
    // — a worktree session spawns at the project root.
    command.args(["acp", "--cwd", session_cwd]);

    let mut child = command
        .current_dir(cwd)
        .env("DRAY_SESSION_ID", session_id)
        .env("PATH", crate::harness::agent_path(&bin))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't start opencode")?;

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

    tokio::spawn({
        let events = events.clone();
        let status = status.clone();
        let queued = queued.clone();
        let seq = seq.clone();
        async move {
            if let Err(error) =
                read_stdout(stdout, reader, ready_rx, events, status, queued, seq).await
            {
                eprintln!("Failed to read opencode stdout: {error}");
            }
        }
    });

    tokio::spawn(async move {
        if let Err(error) = read_stderr(Opencode, stderr).await {
            eprintln!("Failed to read opencode stderr: {error}");
        }
    });

    let (opencode_id, config) =
        match open_session(&client, session_id, session_cwd, is_new_session).await {
            Ok(opened) => opened,
            Err(error) => {
                // Post-spawn, so the child is running with nobody left to talk
                // to it. A `Child` is not reaped on drop.
                let _ = child.kill().await;
                return Err(error);
            }
        };

    let session = OpencodeSession {
        client,
        id: opencode_id,
        prompt_id: Arc::new(std::sync::Mutex::new(None)),
        preamble: Arc::new(AtomicBool::new(owes_preamble(is_new_session, seq_start))),
    };

    // The model on **both** paths, where fx sets it on a resume alone: there is
    // no `--model` to ride the spawn here, so a creation that skipped this
    // would run on whatever opencode's config names and silently disagree with
    // the picker that sent it.
    //
    // **Not fatal.** A model the reader's providers no longer serve would
    // otherwise make its own session unopenable — the failure DRA-221 left in
    // fx's effort path — so the refusal is reported in the transcript and the
    // session runs on opencode's default, with `landed` below recording what
    // that actually is.
    let mut config = config;
    if let Some(model) = model {
        match set_model(&session, model).await {
            Ok(answer) => config = answer,
            Err(error) => {
                let refusal = format!(
                    "opencode would not run {}: {error}. This session is running on its own default model instead.",
                    model.arg
                );
                crate::session::report_session_error(
                    session_id, Opencode, &refusal, &seq, &events, app,
                )
                .await;
            }
        }
    }

    // A stance that will not apply is the fatal one: the session would run
    // freer or narrower than the reader asked, which is not something to report
    // and carry on from.
    if let Err(error) = set_mode(&session, permission_mode).await {
        let _ = child.kill().await;
        return Err(error);
    }

    let landed = landed_model(&config);
    let _ = ready_tx.send(session.clone());

    Ok(Session {
        id: session_id.to_string(),
        child,
        stdin: Transport::Opencode(session),
        harness: Opencode,
        // What opencode says it is on, not what was asked for.
        model: landed.unwrap_or_default(),
        // No level exists to record: opencode publishes none and takes none.
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

/// `initialize`, then `session/new` or `session/load`. Answers opencode's own
/// id and the options the session opened with.
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
                // No `fs` and no `terminal`: opencode reads and writes through
                // its own tools, and advertising either would invite requests
                // this build cannot serve.
                "clientCapabilities": {},
                "clientInfo": {"name": "dray", "title": "Dray", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;

    // Empty, and deliberately so. opencode merges the reader's own MCP config
    // the way its TUI does — `opencode mcp` is its surface for that — where fx
    // reads none of its own and has to be handed the list. A list sent here
    // would be a second source for servers opencode already knows about.
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
        .context("this session has no opencode session to resume")?;

    // `session/load`, not fx's `session/resume`: opencode advertises
    // `loadSession` and answers the standard method, and its own
    // `sessionCapabilities.resume` is a different surface this does not need.
    let answer = client
        .request(
            "session/load",
            json!({"sessionId": recorded, "cwd": session_cwd, "mcpServers": mcp_servers}),
        )
        .await?;

    Ok((recorded, parser::ConfigOptions::of(&answer)))
}

/// Moves a live session onto a model, answering the options it reports back.
///
/// In place, with no respawn: the config option is what the reader's own TUI
/// moves, and the reply restates every option so the caller can record what
/// actually landed.
pub async fn set_model(session: &OpencodeSession, model: &Model) -> Result<parser::ConfigOptions> {
    set_config(session, "model", &model.arg).await
}

/// Which of opencode's two session modes a stance lands on.
///
/// `build` and `plan` are the whole list — measured, `session/new` answers
/// exactly those two — so every stance that is not read-only lands on `build`.
/// **What a session asks permission for is not this**: that is the `permission`
/// block in the reader's own opencode config, which has no per-session surface
/// on ACP at all. So `manual` is honoured only as far as their config already
/// honours it, which is why the composer draws the narrower stances as
/// unhonoured rather than claiming them.
pub fn mode_for(mode: ApprovalPolicy) -> &'static str {
    match mode {
        ApprovalPolicy::Plan => "plan",
        _ => "build",
    }
}

/// Moves a live session's stance.
pub async fn set_mode(session: &OpencodeSession, mode: ApprovalPolicy) -> Result<()> {
    set_config(session, "mode", mode_for(mode)).await.map(|_| ())
}

/// One `session/set_config_option`.
///
/// The field is **`configId`**, which is the one name here worth pinning: the
/// ACP draft calls it `optionId` and opencode answers that with `Invalid input:
/// expected string, received undefined` — a refusal that names the field it
/// wanted and nothing about the option that was being set.
async fn set_config(
    session: &OpencodeSession,
    config_id: &str,
    value: &str,
) -> Result<parser::ConfigOptions> {
    let answer = session
        .client
        .request(
            "session/set_config_option",
            json!({"sessionId": session.id, "configId": config_id, "value": value}),
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
/// **Behind, not in front.** fx measured what leading with 4KB of rules costs —
/// a session titled after Dray rather than after the work — and opencode
/// derives its own title from the first turn the same way. The rules are
/// honoured from down there.
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
/// away, and the read loop settles it on the id kept here. A prompt refused
/// outright still answers on that id, as an error, which the reader draws as a
/// failed turn.
///
/// Only the *transport* text carries the rules: [`crate::session`] logs
/// `user_message` from the reader's own string, so the transcript never sees
/// the block and there is nothing to strip on the way out.
pub async fn start_turn(
    session: &OpencodeSession,
    text: &str,
    images: &[crate::attachments::PreparedImage],
) -> Result<()> {
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
    let mut running = session
        .prompt_id
        .lock()
        .expect("opencode prompt id poisoned");
    let id = session.client.request_detached(
        "session/prompt",
        json!({
            "sessionId": session.id,
            "prompt": crate::attachments::acp_prompt_blocks(&sent, images),
        }),
    )?;
    *running = Some(id);
    session
        .preamble
        .store(false, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Stops the running turn. A notification: nothing is acknowledged, the running
/// tool is killed, and the prompt answers `cancelled` — which is what closes the
/// turn, so the stop is reported through the same path a finished one is.
pub fn cancel(session: &OpencodeSession) -> Result<()> {
    session
        .client
        .notify("session/cancel", json!({"sessionId": session.id}))
}

/// Ends the child cleanly: `session/close`, EOF, then a kill if it lingers.
///
/// `session/close` is advertised on the handshake (`sessionCapabilities.close`)
/// and is what lets opencode finish writing the session it persists — every ACP
/// session it opens is filed under `~/.opencode`, prompted or not.
pub async fn shutdown(child: &mut Child, session: &OpencodeSession) {
    let _ = session
        .client
        .request_within(
            "session/close",
            json!({"sessionId": session.id}),
            SHUTDOWN_GRACE,
        )
        .await;
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
    ready: tokio::sync::oneshot::Receiver<OpencodeSession>,
    events: Arc<Mutex<Vec<AgentEvent>>>,
    status: Arc<Mutex<StatusTracker>>,
    queued: QueuedMessages,
    seq: Arc<AtomicU64>,
) -> Result<()> {
    let mut lines = BufReader::new(stdout).lines();
    let mut mapper = mapper::Mapper::new(handles.session_id.clone(), seq.clone());

    // Held until the session exists. Lines before it are routed — the
    // handshake's answers have to reach their waiters — but mapped to nothing.
    let mut ready = Some(ready);
    let mut transport: Option<Transport> = None;

    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            // stdout closed (the child exited) or a read error — either way no
            // more of this turn is coming. Break to the cleanup below.
            Ok(None) => break,
            Err(err) => {
                eprintln!("[opencode stdout err] {err}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        if transport.is_none() {
            if let Some(rx) = &mut ready {
                match rx.try_recv() {
                    Ok(session) => {
                        transport = Some(Transport::Opencode(session));
                        ready = None;
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => ready = None,
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                }
            }
        }

        // The prompt's own answer, read ahead of the demux: nothing waits on
        // it, so `accept` would file it as stray.
        let mut event: Option<parser::OpencodeEvent> = None;
        if let Some(Transport::Opencode(session)) = transport.as_ref() {
            if let Some(done) = prompt_answer(session, &line) {
                event = Some(done);
            }
        }

        let event = match event {
            Some(event) => event,
            None => match handles.client.accept(&line).await {
                Incoming::Notification { method, params } => {
                    match parser::parse_notification(&method, params) {
                        Ok(parser::OpencodeEvent::Unknown) => {
                            record_failure(Opencode, &handles.session_id, "unknown_method", &method, &line)
                                .await;
                            continue;
                        }
                        Ok(event) => event,
                        Err(err) => {
                            record_failure(Opencode, &handles.session_id, "map", &err.to_string(), &line)
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
                            record_failure(Opencode, &handles.session_id, "unsupported_request", &err.to_string(), &line)
                                .await;
                            let _ = handles
                                .client
                                .respond(id, json!({"outcome": {"outcome": "cancelled"}}));
                        }
                    } else {
                        // `fs/*` and `terminal/*` were not advertised, so one
                        // arriving is opencode asking past the capabilities it
                        // was given. A protocol error leaves it to decide.
                        record_failure(Opencode, &handles.session_id, "unsupported_request", &method, &line)
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
                    record_failure(Opencode, &handles.session_id, "stray_response", &detail, &line).await;
                    continue;
                }
                Incoming::Response { .. } => continue,

                Incoming::Malformed => {
                    record_failure(Opencode, &handles.session_id, "parse", "not a JSON-RPC message", &line)
                        .await;
                    continue;
                }
            },
        };

        let Some(transport) = transport.as_ref() else {
            continue;
        };

        // A published command list is recorded rather than drawn: it arrives
        // on every `session/new`, and the `/` picker is the only thing that
        // wants it. See `commands.rs` for why this is the only source.
        if let parser::OpencodeEvent::Update(parser::SessionUpdate::AvailableCommandsUpdate {
            available_commands,
        }) = &event
        {
            commands::remember(&handles.session_cwd, available_commands.clone());
        }

        let ingest = crate::session::Ingest {
            session_id: &handles.session_id,
            harness: Opencode,
            session_cwd: &handles.session_cwd,
            events: &events,
            status: &status,
            queued: &queued,
            flush_seq: &seq,
            flush_events: &events,
            flush_transport: transport,
        };

        for agent_event in mapper.map(event) {
            crate::session::ingest(&ingest, agent_event, &handles.app).await;
        }
    }

    // The child's stdout has ended. If a prompt was still in flight its answer
    // will never arrive, so close the turn as a failure — without which the
    // session hangs `in_progress` forever, its queue with no boundary to drain
    // at. The queue is stranded *first* (reported and cleared), so the closing
    // turn's boundary flush finds nothing to hand the dead child.
    if let Some(transport @ Transport::Opencode(session)) = transport.as_ref() {
        // Clear it either way — the child is gone, so a Stop pressed now names
        // nothing — and remember whether a turn was open to close it below.
        let outstanding = session
            .prompt_id
            .lock()
            .expect("opencode prompt id poisoned")
            .take()
            .is_some();

        crate::session::strand_queue_on_exit(
            &handles.session_id,
            Opencode,
            &queued,
            &seq,
            &events,
            &handles.app,
        )
        .await;

        if outstanding {
            let ingest = crate::session::Ingest {
                session_id: &handles.session_id,
                harness: Opencode,
                session_cwd: &handles.session_cwd,
                events: &events,
                status: &status,
                queued: &queued,
                flush_seq: &seq,
                flush_events: &events,
                flush_transport: transport,
            };
            for agent_event in mapper.map(parser::OpencodeEvent::PromptFailed {
                message: "opencode exited before finishing this turn.".to_string(),
            }) {
                crate::session::ingest(&ingest, agent_event, &handles.app).await;
            }
        }
    }

    Ok(())
}

/// The running prompt's response, where `line` is it.
///
/// A response with no method and the prompt's id. Both a result and an error
/// close the turn, and the id is cleared here so a Stop pressed after the
/// answer names nothing.
fn prompt_answer(session: &OpencodeSession, line: &str) -> Option<parser::OpencodeEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("method").is_some() {
        return None;
    }
    let id = value.get("id").and_then(Value::as_i64)?;

    let mut running = session.prompt_id.lock().expect("opencode prompt id poisoned");
    if *running != Some(id) {
        return None;
    }
    *running = None;

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("opencode could not run this prompt")
            .to_string();
        return Some(parser::OpencodeEvent::PromptFailed { message });
    }

    let response: parser::PromptResponse = value
        .get("result")
        .cloned()
        .and_then(|r| serde_json::from_value(r).ok())
        .unwrap_or_default();
    Some(parser::OpencodeEvent::PromptDone(response))
}

/// Turns one permission request into the card that answers it.
///
/// Registered before it is emitted, so a button pressed the instant the card
/// draws finds the entry waiting. The reply goes out from
/// [`Session::respond_permission`](crate::session::Session::respond_permission)
/// when the user picks; until then opencode is blocked.
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
    let command = input.get("command").and_then(Value::as_str).map(str::to_string);
    let path = input.get("path").and_then(Value::as_str).map(str::to_string);

    let event = mapper.synthesize(AgentEventPayload::PermissionRequested {
        request_id,
        tool_use_id: request.tool_call.tool_call_id.clone(),
        tool_name: request
            .tool_call
            .name
            .clone()
            .unwrap_or_else(|| "tool".to_string()),
        display_name: None,
        // The command or the path, which is what the card draws as the
        // subject. opencode's own `title` is the tool name with the command
        // after it, which the row above already says.
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
