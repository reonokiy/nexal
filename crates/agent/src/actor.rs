//! Agent Actor — non-blocking message processing with inbox + event stream.
//!
//! Each session gets one `AgentActor` that runs in a background task.
//! Messages arrive via `AgentHandle::send()` (non-blocking).
//! Responses flow out via an event callback.

use std::sync::Arc;

use nexal_app_server_client::AppServerClient;
use nexal_app_server_client::AppServerEvent;
use nexal_app_server_protocol::AskForApproval as ApiAskForApproval;
use nexal_app_server_protocol::ClientRequest;
use nexal_app_server_protocol::RequestId;
use nexal_app_server_protocol::SandboxPolicy as ApiSandboxPolicy;
use nexal_app_server_protocol::ServerNotification;
use nexal_app_server_protocol::TurnStartParams;
use nexal_app_server_protocol::UserInput;
use nexal_config::NexalConfig;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use crate::signal::StateSignal;

use crate::runner::reject_all_server_requests;

/// Message types that can be sent to an agent actor.
#[derive(Debug)]
pub enum AgentMessage {
    /// New user input.
    UserInput {
        text: String,
        sender: String,
        channel: String,
        chat_id: String,
        metadata: serde_json::Value,
        images: Vec<nexal_channel_core::ImageAttachment>,
    },
    /// Interrupt current work.
    Interrupt,
    /// Shutdown the actor.
    Shutdown,
}

/// Response events emitted by an agent actor.
///
/// Note: there is no `Response { text }` variant because the agent **never**
/// replies through this event stream — every channel's reply path runs
/// inside the sandbox container as a skill script that POSTs to a host-side
/// Unix-socket proxy. The bot event consumer only watches these events to
/// drive UI side effects (typing indicators, error logging).
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent encountered an error.
    Error {
        session_key: String,
        message: String,
    },
    /// Agent status changed (for dashboard + typing indicators).
    StatusChange {
        session_key: String,
        status: String,
        activity: String,
    },
}

/// Handle to send messages to a running agent actor.
#[derive(Clone)]
pub struct AgentHandle {
    tx: mpsc::Sender<AgentMessage>,
}

impl AgentHandle {
    /// Send a message to the actor (non-blocking).
    pub async fn send(&self, msg: AgentMessage) -> anyhow::Result<()> {
        self.tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("agent actor is dead"))
    }
}

/// Result of draining a single turn.
struct DrainResult {
    /// Accumulated plain-text output from the model.
    text: String,
    /// Whether any tool call (of any kind) was executed.
    had_tool_call: bool,
    /// Whether a "response action" was taken — i.e. a tool call that counts
    /// as the agent having made a deliberate send-or-skip decision.
    had_response_action: bool,
    /// True if the turn was interrupted by an `AgentMessage::Interrupt`.
    interrupted: bool,
    /// Messages that arrived while the turn was active and could not be
    /// processed in-place. The caller should replay these after the turn.
    buffered: Vec<AgentMessage>,
}

/// A running agent actor.
pub(crate) struct AgentActor {
    session_key: String,
    client: AppServerClient,
    thread_id: String,
    config: Arc<NexalConfig>,
    /// Receiver for push-based state signals from tool scripts.
    signal_rx: Option<broadcast::Receiver<StateSignal>>,
}

impl AgentActor {
    pub(crate) fn new(
        session_key: String,
        client: AppServerClient,
        thread_id: String,
        config: Arc<NexalConfig>,
        signal_rx: Option<broadcast::Receiver<StateSignal>>,
    ) -> Self {
        Self {
            session_key,
            client,
            thread_id,
            config,
            signal_rx,
        }
    }

    /// Spawn the actor as a background task. Returns a handle for sending messages.
    pub(crate) fn spawn(
        self,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> AgentHandle {
        let (tx, rx) = mpsc::channel::<AgentMessage>(32);
        tokio::spawn(self.run(rx, event_tx));
        AgentHandle { tx }
    }

    /// Build `TurnStartParams` with the same policy every turn uses.
    ///
    /// The agent always runs inside the container: cwd is fixed to the
    /// container-side `/workspace`, approval is disabled, and the sandbox
    /// is `WorkspaceWrite`. Only `input` varies between turns.
    fn build_turn_params(&self, input: Vec<UserInput>) -> TurnStartParams {
        TurnStartParams {
            thread_id: self.thread_id.clone(),
            input,
            cwd: Some(std::path::PathBuf::from("/workspace")),
            approval_policy: Some(ApiAskForApproval::Never),
            sandbox_policy: Some(ApiSandboxPolicy::WorkspaceWrite {
                writable_roots: vec![],
                read_only_access: Default::default(),
                network_access: self.config.sandbox_network,
                exclude_tmpdir_env_var: false,
                exclude_slash_tmp: false,
            }),
            ..Default::default()
        }
    }

    /// Main event loop.
    async fn run(
        mut self,
        mut inbox: mpsc::Receiver<AgentMessage>,
        event_tx: mpsc::Sender<AgentEvent>,
    ) {
        info!(session = %self.session_key, "agent actor started");

        // Messages that arrived while a turn was active are buffered here
        // and replayed before pulling the next message from inbox.
        let mut lookahead: std::collections::VecDeque<AgentMessage> =
            std::collections::VecDeque::new();

        loop {
            let msg = if let Some(queued) = lookahead.pop_front() {
                queued
            } else {
                match inbox.recv().await {
                    Some(m) => m,
                    None => break,
                }
            };

            match msg {
                AgentMessage::UserInput {
                    text,
                    sender,
                    channel,
                    chat_id,
                    metadata,
                    images,
                } => {
                    let buffered = self
                        .handle_input(text, sender, channel, chat_id, metadata, images, &mut inbox, &event_tx)
                        .await;
                    lookahead.extend(buffered);
                }
                AgentMessage::Interrupt => {
                    // No active turn — nothing to interrupt.
                    debug!(session = %self.session_key, "interrupt received with no active turn; ignoring");
                }
                AgentMessage::Shutdown => {
                    info!(session = %self.session_key, "agent actor shutting down");
                    break;
                }
            }
        }
    }

    /// Process a user input: start a turn, drain events, emit response.
    /// Returns any `AgentMessage`s that arrived while a turn was active and
    /// could not be processed in-place. The caller should replay them.
    async fn handle_input(
        &mut self,
        text: String,
        sender: String,
        channel: String,
        chat_id: String,
        metadata: serde_json::Value,
        images: Vec<nexal_channel_core::ImageAttachment>,
        inbox: &mut mpsc::Receiver<AgentMessage>,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) -> Vec<AgentMessage> {
        let prompt_text = render_channel_context(&text, &sender, &channel, &chat_id, &metadata);
        info!(
            session = %self.session_key,
            thread = %self.thread_id,
            input_len = prompt_text.len(),
            "starting agent turn"
        );
        // Signal: working — write to state file for live status bar
        let _ = event_tx
            .send(AgentEvent::StatusChange {
                session_key: self.session_key.clone(),
                status: "working".into(),
                activity: truncate(&text, 40),
            })
            .await;
        write_agent_status(&self.config, "working", &truncate(&text, 40));

        use nexal_app_server_protocol::TurnStartResponse;

        let mut input = vec![UserInput::Text {
            text: prompt_text,
            text_elements: vec![],
        }];
        for img in &images {
            let data_url = compress_and_encode_image(&img.data, &img.mime_type);
            input.push(UserInput::Image { url: data_url });
        }

        let turn_result: Result<TurnStartResponse, _> = self
            .client
            .request_typed(ClientRequest::TurnStart {
                request_id: RequestId::Integer(1),
                params: self.build_turn_params(input),
            })
            .await;

        let turn_resp = match turn_result {
            Ok(resp) => resp,
            Err(e) => {
                let _ = event_tx
                    .send(AgentEvent::Error {
                        session_key: self.session_key.clone(),
                        message: format!("turn/start: {e}"),
                    })
                    .await;
                return Vec::new();
            }
        };

        let turn_id = turn_resp.turn.id.clone();
        debug!(
            session = %self.session_key,
            task = %turn_id,
            "turn started"
        );

        // Drain events until turn completes
        let mut result = self.drain_turn(inbox, &turn_id).await;
        let mut all_buffered = result.buffered;

        if result.interrupted {
            let _ = event_tx
                .send(AgentEvent::StatusChange {
                    session_key: self.session_key.clone(),
                    status: "idle".into(),
                    activity: String::new(),
                })
                .await;
            write_agent_status(&self.config, "idle", "");
            return all_buffered;
        }

        // ── Response-action state machine ──
        // The agent is now "busy". It must transition back to "idle" by taking
        // a response action: telegram_send, telegram_edit, a reaction API call,
        // or the explicit no_response script.  If none of these happened, nudge
        // the model up to MAX_NUDGES times.
        const MAX_NUDGES: usize = 2;
        let mut nudge_count = 0;

        while !result.had_response_action && nudge_count < MAX_NUDGES {
            nudge_count += 1;

            let reason = if !result.had_tool_call && !result.text.trim().is_empty() {
                // Model produced plain text but no tool calls at all.
                "Your previous response was plain text and was NOT delivered to the \
                 user. In headless mode, plain text is silently discarded."
            } else if result.had_tool_call {
                // Model made tool calls (e.g. reading files, thinking) but never
                // actually sent a message or called no_response.
                "You executed tool calls but did NOT send a message to the user and \
                 did NOT call no_response.sh. The user is still waiting."
            } else {
                // No tool calls and no text — model just returned empty.
                "You completed a turn without any output or action. The user sent a \
                 message and is waiting for a response."
            };
            let nudge_msg = format!(
                "[system] {reason}\n\n\
                 You MUST take one of these actions:\n\
                 1. Send a reply: exec_command with telegram_send.py\n\
                 2. Explicitly skip: exec_command with `./scripts/no_response.sh`\n\n\
                 Do one of the above NOW. (nudge {nudge_count}/{MAX_NUDGES})"
            );

            warn!(
                session = %self.session_key,
                thread = %self.thread_id,
                nudge = nudge_count,
                had_tool_call = result.had_tool_call,
                had_text = !result.text.trim().is_empty(),
                "no response action taken — sending nudge"
            );

            let retry: Result<TurnStartResponse, _> = self
                .client
                .request_typed(ClientRequest::TurnStart {
                    request_id: RequestId::Integer(1 + nudge_count as i64),
                    params: self.build_turn_params(vec![UserInput::Text {
                        text: nudge_msg,
                        text_elements: vec![],
                    }]),
                })
                .await;

            match retry {
                Ok(resp) => {
                    let nudge_turn_id = resp.turn.id.clone();
                    result = self.drain_turn(inbox, &nudge_turn_id).await;
                    all_buffered.extend(result.buffered.drain(..));
                    if result.interrupted {
                        break;
                    }
                }
                Err(e) => {
                    warn!(session = %self.session_key, "nudge turn failed: {e}");
                    break;
                }
            }
        }

        if !result.had_response_action {
            warn!(
                session = %self.session_key,
                thread = %self.thread_id,
                nudges = nudge_count,
                "agent never took a response action after all nudges"
            );
        }

        // Agent replies always flow through in-container skill scripts,
        // never through this event stream. All we need to emit is the
        // idle transition so the UI can drop the typing indicator.
        let _ = event_tx
            .send(AgentEvent::StatusChange {
                session_key: self.session_key.clone(),
                status: "idle".into(),
                activity: String::new(),
            })
            .await;
        write_agent_status(&self.config, "idle", "");
        all_buffered
    }

    /// Drain the event stream until `TurnCompleted`, `Error`, or an interrupt.
    ///
    /// Polls `inbox` inside the select so that an `AgentMessage::Interrupt`
    /// arriving mid-turn sends `TurnInterrupt` to the server immediately and
    /// returns with `DrainResult::interrupted = true`.
    async fn drain_turn(
        &mut self,
        inbox: &mut mpsc::Receiver<AgentMessage>,
        active_turn_id: &str,
    ) -> DrainResult {
        use nexal_app_server_protocol::{TurnInterruptParams, TurnInterruptResponse};

        let mut buf = String::new();
        let mut had_any_tool_call = false;
        let mut had_response_action = false;
        let mut interrupted = false;
        let mut buffered: Vec<AgentMessage> = Vec::new();
        let thread_id = &self.thread_id;

        // Timeout to avoid hanging forever if API silently fails
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(120));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                event = self.client.next_event() => {
                    match event {
                        None => break,
                        Some(AppServerEvent::ServerNotification(notif)) => match notif {
                            ServerNotification::AgentMessageDelta(delta)
                                if delta.thread_id == *thread_id =>
                            {
                                buf.push_str(&delta.delta);
                                tracing::trace!(
                                    session = %self.session_key,
                                    delta_len = delta.delta.len(),
                                    buffered_len = buf.len(),
                                    "received agent message delta"
                                );
                                // Reset timeout on activity
                                timeout.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_secs(120));
                            }
                            ServerNotification::TurnCompleted(completed)
                                if completed.thread_id == *thread_id =>
                            {
                                info!(
                                    session = %self.session_key,
                                    turn_id = %completed.turn.id,
                                    "received turn completed notification"
                                );
                                break;
                            }
                            ServerNotification::Error(err)
                                if err.thread_id == *thread_id =>
                            {
                                warn!("agent error: {}", err.error.message);
                                if !err.will_retry {
                                    buf.push_str(&format!("\n[error: {}]", err.error.message));
                                    break;
                                }
                            }
                            ServerNotification::ItemCompleted(item_completed)
                                if item_completed.thread_id == *thread_id =>
                            {
                                // Track if any command was executed (regardless of result).
                                if matches!(
                                    &item_completed.item,
                                    nexal_app_server_protocol::ThreadItem::CommandExecution { .. }
                                ) {
                                    had_any_tool_call = true;
                                }
                            }
                            other => {
                                debug!(
                                    session = %self.session_key,
                                    notification = ?other,
                                    "received unhandled server notification"
                                );
                            }
                        },
                        Some(AppServerEvent::ServerRequest(req)) => {
                            warn!(
                                session = %self.session_key,
                                request = ?req,
                                "received server request in headless mode; rejecting"
                            );
                            reject_all_server_requests(&self.client, req).await;
                        }
                        Some(AppServerEvent::Lagged { skipped }) => {
                            warn!("event stream lagged, skipped {skipped} events");
                        }
                        Some(AppServerEvent::Disconnected { message }) => {
                            warn!("client disconnected: {message}");
                            break;
                        }
                    }
                }
                // Push-based state signal from tool scripts via Unix socket.
                signal = async {
                    match self.signal_rx {
                        Some(ref mut rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok(sig) = signal {
                        if sig.session == self.session_key && sig.state == "IDLE" {
                            info!(
                                session = %self.session_key,
                                "received IDLE signal from tool script"
                            );
                            had_response_action = true;
                        }
                    }
                }
                _ = &mut timeout => {
                    warn!(session = %self.session_key, "drain_turn timed out after 120s");
                    if buf.is_empty() {
                        buf.push_str("[timeout: no response from model]");
                    }
                    break;
                }
                msg = inbox.recv() => {
                    match msg {
                        Some(AgentMessage::Interrupt) => {
                            info!(
                                session = %self.session_key,
                                turn = %active_turn_id,
                                "interrupting active turn"
                            );
                            let _ = self.client
                                .request_typed::<TurnInterruptResponse>(
                                    ClientRequest::TurnInterrupt {
                                        request_id: RequestId::Integer(0),
                                        params: TurnInterruptParams {
                                            thread_id: self.thread_id.clone(),
                                            turn_id: active_turn_id.to_string(),
                                        },
                                    },
                                )
                                .await;
                            interrupted = true;
                            break;
                        }
                        Some(AgentMessage::Shutdown) => {
                            // Propagate shutdown: interrupt the current turn first.
                            let _ = self.client
                                .request_typed::<TurnInterruptResponse>(
                                    ClientRequest::TurnInterrupt {
                                        request_id: RequestId::Integer(0),
                                        params: TurnInterruptParams {
                                            thread_id: self.thread_id.clone(),
                                            turn_id: active_turn_id.to_string(),
                                        },
                                    },
                                )
                                .await;
                            interrupted = true;
                            break;
                        }
                        Some(msg) => {
                            // Buffer UserInput that arrived mid-turn; the run
                            // loop will replay them once the turn finishes.
                            debug!(
                                session = %self.session_key,
                                "buffering message received during active turn"
                            );
                            buffered.push(msg);
                        }
                        None => break,
                    }
                }
            }
        }

        DrainResult {
            text: buf,
            had_tool_call: had_any_tool_call,
            had_response_action,
            interrupted,
            buffered,
        }
    }
}

/// Write agent status to a state file for live TUI status bar rendering.
/// The TUI reads this file every frame to show current agent state.
fn write_agent_status(config: &NexalConfig, status: &str, activity: &str) {
    let state_file = config.workspace.join("agents").join(".agent_status");
    let content = if activity.is_empty() {
        status.to_string()
    } else {
        format!("{status}: {activity}")
    };
    // Fire-and-forget on the blocking threadpool so the tokio worker is not stalled.
    tokio::task::spawn_blocking(move || {
        let _ = std::fs::write(state_file, content);
    });
}

/// Truncate `s` to at most `max_chars` Unicode characters, appending `...`
/// if any were dropped.
fn truncate(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

fn render_channel_context(
    text: &str,
    sender: &str,
    channel: &str,
    chat_id: &str,
    metadata: &serde_json::Value,
) -> String {
    if channel.is_empty() || chat_id.is_empty() {
        return text.to_string();
    }

    let mut out = format!(
        "[channel={channel} sender={sender} chat_id={chat_id}",
    );

    if !metadata.is_null() {
        out.push_str(&format!(" metadata={metadata}"));
    }

    out.push_str("]\n");
    out.push_str(text);
    out
}


/// Compress an image to a max dimension of 768px and encode as a data URI.
/// This keeps images small enough for LLM vision context while still readable.
fn compress_and_encode_image(data: &[u8], mime_type: &str) -> String {
    use base64::Engine;

    const MAX_DIM: u32 = 768;
    const JPEG_QUALITY: u8 = 60;

    // Try to decode, resize, and re-encode as JPEG.
    // Fall back to the raw bytes with the original MIME type on any failure.
    if let Ok(img) = image::load_from_memory(data) {
        let resized = img.resize(MAX_DIM, MAX_DIM, image::imageops::FilterType::Triangle);
        let mut buf = std::io::Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
        if resized.write_with_encoder(encoder).is_ok() {
            let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
            return format!("data:image/jpeg;base64,{b64}");
        }
    }

    // Fallback: pass through raw bytes with the original MIME type so the
    // caller gets a correctly-labelled data URI instead of lying about jpeg.
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    format!("data:{mime_type};base64,{b64}")
}
