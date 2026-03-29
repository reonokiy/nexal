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

use crate::podman::PodmanContainer;
use crate::runner::{reject_all_server_requests, workspace_cwd};
use crate::split_response;

/// Message types that can be sent to an agent actor.
#[derive(Debug)]
pub enum AgentMessage {
    /// New user input.
    UserInput {
        text: String,
        sender: String,
        channel: String,
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
    /// Create a handle from a raw sender (for testing).
    pub fn new_from_sender(tx: mpsc::Sender<AgentMessage>) -> Self {
        Self { tx }
    }

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
    #[allow(dead_code)]
    container: Option<PodmanContainer>,
}

impl AgentActor {
    pub(crate) fn new(
        session_key: String,
        client: AppServerClient,
        thread_id: String,
        config: Arc<NexalConfig>,
        container: Option<PodmanContainer>,
    ) -> Self {
        Self {
            session_key,
            client,
            thread_id,
            config,
            container,
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
                AgentMessage::UserInput { text, .. } => {
                    self.handle_input(text, &event_tx).await;
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
        event_tx: &mpsc::Sender<AgentEvent>,
    ) {
        // Signal: working — write to state file for live status bar
        let _ = event_tx
            .send(AgentEvent::StatusChange {
                session_key: self.session_key.clone(),
                status: "working".into(),
                activity: truncate(&text, 40),
            })
            .await;
        write_agent_status(&self.config, "working", &truncate(&text, 40));

        let cwd = workspace_cwd(&self.config);

        use nexal_app_server_protocol::TurnStartResponse;

        let turn_result: Result<TurnStartResponse, _> = self
            .client
            .request_typed(ClientRequest::TurnStart {
                request_id: RequestId::Integer(1),
                params: TurnStartParams {
                    thread_id: self.thread_id.clone(),
                    input: vec![UserInput::Text {
                        text,
                        text_elements: vec![],
                    }],
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

        let task_id = turn_resp.turn.id;
        debug!(
            session = %self.session_key,
            task = %task_id,
            "turn started"
        );

        // Drain events until turn completes
        let response_buf = self.drain_turn(&task_id).await;

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
    async fn drain_turn(&mut self, task_id: &str) -> String {
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
                                // Reset timeout on activity
                                timeout.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_secs(120));
                            }
                            ServerNotification::TurnCompleted(completed)
                                if completed.thread_id == *thread_id =>
                            {
                                debug!("turn completed");
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
                            _ => {}
                        },
                        Some(AppServerEvent::ServerRequest(req)) => {
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
        format!("{}...", &s[..max])
    }
}
