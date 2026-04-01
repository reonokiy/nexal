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
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::runner::reject_all_server_requests;
use crate::split_response;

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
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent produced text response (split into chunks).
    Response {
        session_key: String,
        chunks: Vec<String>,
        thread_id: String,
    },
    /// Agent encountered an error.
    Error {
        session_key: String,
        message: String,
    },
    /// Agent status changed (for dashboard).
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

/// A running agent actor.
pub(crate) struct AgentActor {
    session_key: String,
    client: AppServerClient,
    thread_id: String,
    config: Arc<NexalConfig>,
}

impl AgentActor {
    pub(crate) fn new(
        session_key: String,
        client: AppServerClient,
        thread_id: String,
        config: Arc<NexalConfig>,
    ) -> Self {
        Self {
            session_key,
            client,
            thread_id,
            config,
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

    /// Main event loop.
    async fn run(
        mut self,
        mut inbox: mpsc::Receiver<AgentMessage>,
        event_tx: mpsc::Sender<AgentEvent>,
    ) {
        info!(session = %self.session_key, "agent actor started");

        while let Some(msg) = inbox.recv().await {
            match msg {
                AgentMessage::UserInput {
                    text,
                    sender,
                    channel,
                    chat_id,
                    metadata,
                    images,
                } => {
                    self.handle_input(text, sender, channel, chat_id, metadata, images, &event_tx)
                        .await;
                }
                AgentMessage::Interrupt => {
                    debug!(session = %self.session_key, "interrupt requested");
                    // TODO: send Op::Interrupt via client
                }
                AgentMessage::Shutdown => {
                    info!(session = %self.session_key, "agent actor shutting down");
                    break;
                }
            }
        }
    }

    /// Process a user input: start a turn, drain events, emit response.
    async fn handle_input(
        &mut self,
        text: String,
        sender: String,
        channel: String,
        chat_id: String,
        metadata: serde_json::Value,
        images: Vec<nexal_channel_core::ImageAttachment>,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) {
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

        // Use the container-side path. The host workspace is mounted at
        // /workspace inside the container — the agent must never see host paths.
        let cwd = std::path::PathBuf::from("/workspace");

        use nexal_app_server_protocol::TurnStartResponse;

        let turn_result: Result<TurnStartResponse, _> = self
            .client
            .request_typed(ClientRequest::TurnStart {
                request_id: RequestId::Integer(1),
                params: TurnStartParams {
                    thread_id: self.thread_id.clone(),
                    input: {
                        let mut items = vec![UserInput::Text {
                            text: prompt_text,
                            text_elements: vec![],
                        }];
                        for img in &images {
                            let data_url = compress_and_encode_image(&img.data, &img.mime_type);
                            items.push(UserInput::Image { url: data_url });
                        }
                        items
                    },
                    cwd: Some(cwd),
                    approval_policy: Some(ApiAskForApproval::Never),
                    sandbox_policy: Some(ApiSandboxPolicy::WorkspaceWrite {
                        writable_roots: vec![],
                        read_only_access: Default::default(),
                        network_access: self.config.sandbox_network,
                        exclude_tmpdir_env_var: false,
                        exclude_slash_tmp: false,
                    }),
                    ..Default::default()
                },
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
                return;
            }
        };

        debug!(
            session = %self.session_key,
            task = %turn_resp.turn.id,
            "turn started"
        );

        // Drain events until turn completes
        let response_buf = self.drain_turn().await;
        if response_buf.trim().is_empty() {
            warn!(
                session = %self.session_key,
                thread = %self.thread_id,
                "turn completed with empty text response"
            );
        } else {
            info!(
                session = %self.session_key,
                thread = %self.thread_id,
                response_len = response_buf.len(),
                "turn completed with text response"
            );
        }

        let chunks = split_response(response_buf);
        let _ = event_tx
            .send(AgentEvent::Response {
                session_key: self.session_key.clone(),
                chunks,
                thread_id: self.thread_id.clone(),
            })
            .await;

        // Signal: idle
        let _ = event_tx
            .send(AgentEvent::StatusChange {
                session_key: self.session_key.clone(),
                status: "idle".into(),
                activity: String::new(),
            })
            .await;
        write_agent_status(&self.config, "idle", "");
    }

    /// Drain the event stream until `TurnCompleted` or `Error`.
    async fn drain_turn(&mut self) -> String {
        let mut buf = String::new();
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
                _ = &mut timeout => {
                    warn!(session = %self.session_key, "drain_turn timed out after 120s");
                    if buf.is_empty() {
                        buf.push_str("[timeout: no response from model]");
                    }
                    break;
                }
            }
        }

        buf
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
    let _ = std::fs::write(state_file, content);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.char_indices()
            .map(|(i, _)| i)
            .find(|&i| i >= max)
            .unwrap_or(s.len());
        format!("{}...", &s[..end])
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
fn compress_and_encode_image(data: &[u8], _mime_type: &str) -> String {
    use base64::Engine;

    const MAX_DIM: u32 = 768;
    const JPEG_QUALITY: u8 = 60;

    // Try to decode and resize. If decoding fails (unsupported format),
    // fall back to encoding the raw bytes.
    let jpeg_bytes = match image::load_from_memory(data) {
        Ok(img) => {
            let resized = img.resize(MAX_DIM, MAX_DIM, image::imageops::FilterType::Triangle);
            let mut buf = std::io::Cursor::new(Vec::new());
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
            if resized.write_with_encoder(encoder).is_err() {
                buf = std::io::Cursor::new(data.to_vec());
            }
            buf.into_inner()
        }
        Err(_) => data.to_vec(),
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    format!("data:image/jpeg;base64,{b64}")
}
