//! Persistent per-session agent pool.
//!
//! One codex client is kept alive per chat session so the agent's state
//! (conversation history, file watchers, shell environment) persists across
//! messages.  Sessions are created lazily on the first message.
//!
//! The pool supports multiple sandbox backends:
//! - **Podman** (default): runs codex app-server inside a container, connects via WebSocket.
//! - **None**: in-process codex with no sandbox (not recommended).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use nexal_app_server_client::AppServerClient;
use nexal_app_server_client::AppServerEvent;
use nexal_app_server_client::RemoteAppServerConnectArgs;
use nexal_app_server_client::RemoteAppServerClient;
use nexal_app_server_protocol::AskForApproval as ApiAskForApproval;
use nexal_app_server_protocol::ClientRequest;
use nexal_app_server_protocol::RequestId;
use nexal_app_server_protocol::SandboxPolicy as ApiSandboxPolicy;
use nexal_app_server_protocol::ServerNotification;
use nexal_app_server_protocol::TurnStartParams;
use nexal_app_server_protocol::TurnStartResponse;
use nexal_app_server_protocol::UserInput;
use nexal_config::NexalConfig;
use nexal_config::SandboxBackend;
use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::podman::PodmanContainer;
use crate::runner::{
    build_client, build_nexal_config_loader, reject_all_server_requests, start_thread, workspace_cwd,
};
use crate::split_response;

struct LiveSession {
    client: AppServerClient,
    thread_id: String,
    /// Podman container handle (only for podman backend).
    _container: Option<PodmanContainer>,
}

/// A pool of persistent codex agent sessions, one per chat.
///
/// Pass an `Arc<AgentPool>` to each channel adapter.  The pool is
/// thread-safe; concurrent messages on *different* sessions run in parallel.
/// Messages on the *same* session are serialised by a per-session mutex.
pub struct AgentPool {
    config: Arc<NexalConfig>,
    sessions: Mutex<HashMap<String, Arc<Mutex<LiveSession>>>>,
}

impl AgentPool {
    pub fn new(config: Arc<NexalConfig>) -> Arc<Self> {
        info!("agent pool using sandbox backend: {}", config.sandbox_backend());
        Arc::new(Self {
            config,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Run a single user turn and return the response split into ≤4000-char
    /// chunks.  Also returns the thread ID so the caller can persist it.
    ///
    /// `session_key` should be `"channel:chat_id"`, e.g. `"telegram:123456"`.
    pub async fn run_turn(
        &self,
        session_key: &str,
        text: String,
    ) -> anyhow::Result<(Vec<String>, String)> {
        let entry = self.get_or_create(session_key).await?;
        let mut live = entry.lock().await;

        let cwd = workspace_cwd(&self.config);

        let turn_resp: TurnStartResponse = live
            .client
            .request_typed(ClientRequest::TurnStart {
                request_id: RequestId::Integer(1),
                params: TurnStartParams {
                    thread_id: live.thread_id.clone(),
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
            .await
            .map_err(|e| anyhow::anyhow!("turn/start: {e}"))?;

        let task_id = turn_resp.turn.id;
        let thread_id = live.thread_id.clone();
        debug!("turn started: session={session_key} thread={thread_id} task={task_id}");

        let response_buf = drain_turn(&mut live.client, &thread_id, &task_id).await;

        Ok((split_response(response_buf), thread_id))
    }

    async fn get_or_create(
        &self,
        key: &str,
    ) -> anyhow::Result<Arc<Mutex<LiveSession>>> {
        // Fast path: session already exists.
        {
            let map = self.sessions.lock().await;
            if let Some(s) = map.get(key) {
                return Ok(Arc::clone(s));
            }
        }

        // Slow path: spin up a new session.
        info!("creating new agent session for {key}");
        let live = match self.config.sandbox_backend() {
            SandboxBackend::Podman => self.create_podman_session(key).await?,
            SandboxBackend::None => self.create_inprocess_session().await?,
        };
        let new_entry = Arc::new(Mutex::new(live));

        let mut map = self.sessions.lock().await;
        Ok(Arc::clone(map.entry(key.to_string()).or_insert(new_entry)))
    }

    /// Create a session backed by an in-process codex client (bwrap or no sandbox).
    async fn create_inprocess_session(&self) -> anyhow::Result<LiveSession> {
        let soul = self.config.load_soul().await;
        let nexal_config_loader = Arc::new(
            build_nexal_config_loader(&self.config, soul)
                .await
                .context("building codex config")?,
        );
        let mut client = build_client(Arc::clone(&nexal_config_loader)).await?;
        let thread_id = start_thread(&mut client, &nexal_config_loader).await?;
        info!("in-process session ready: thread={thread_id}");

        Ok(LiveSession {
            client: AppServerClient::InProcess(client),
            thread_id,
            _container: None,
        })
    }

    /// Create a session backed by a Podman container running the codex app-server.
    async fn create_podman_session(&self, key: &str) -> anyhow::Result<LiveSession> {
        // Find the app-server binary — use the one built alongside nexal.
        let app_server_bin = find_app_server_binary()?;

        let container = PodmanContainer::start(key, &self.config, &app_server_bin).await?;

        // Wait a moment for the WebSocket server to be ready
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Connect via WebSocket
        let ws_url = container.ws_url();
        let connect_args = RemoteAppServerConnectArgs {
            websocket_url: ws_url.clone(),
            auth_token: None,
            client_name: "nexal".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            experimental_api: true,
            opt_out_notification_methods: vec![],
            channel_capacity: 256,
        };

        let client = retry_connect(connect_args, 10).await
            .context("connecting to podman app-server")?;

        // Start a thread
        let soul = self.config.load_soul().await;
        let nexal_config_loader = Arc::new(
            build_nexal_config_loader(&self.config, soul)
                .await
                .context("building codex config for thread")?,
        );

        // For remote client, we need to use ThreadStart request directly
        let thread_id = start_remote_thread(&client, &nexal_config_loader).await?;
        info!("podman session ready: key={key} container={} thread={thread_id}", container.name);

        Ok(LiveSession {
            client: AppServerClient::Remote(client),
            thread_id,
            _container: Some(container),
        })
    }
}

/// Drain the event stream until `TurnCompleted` or `Error` for `task_id`.
async fn drain_turn(client: &mut AppServerClient, thread_id: &str, task_id: &str) -> String {
    let mut buf = String::new();

    loop {
        match client.next_event().await {
            None => break,

            Some(AppServerEvent::ServerNotification(notif)) => match notif {
                ServerNotification::AgentMessageDelta(delta) if delta.thread_id == thread_id => {
                    buf.push_str(&delta.delta);
                }
                ServerNotification::TurnCompleted(completed)
                    if completed.thread_id == thread_id && completed.turn.id == task_id =>
                {
                    debug!("turn completed");
                    break;
                }
                ServerNotification::Error(err)
                    if err.thread_id == thread_id
                        && err.turn_id == task_id
                        && !err.will_retry =>
                {
                    warn!("agent error: {}", err.error.message);
                    break;
                }
                _ => {}
            },

            Some(AppServerEvent::ServerRequest(req)) => {
                reject_all_server_requests(client, req).await;
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

    buf
}

/// Start a thread on a remote app-server client.
async fn start_remote_thread(
    client: &RemoteAppServerClient,
    config: &nexal_core::config::Config,
) -> anyhow::Result<String> {
    use nexal_app_server_protocol::AskForApproval as ApiAskForApproval;
    use nexal_app_server_protocol::SandboxMode as ApiSandboxMode;
    use nexal_app_server_protocol::ThreadStartParams;
    use nexal_app_server_protocol::ThreadStartResponse;

    let resp: ThreadStartResponse = client
        .request_typed(ClientRequest::ThreadStart {
            request_id: RequestId::Integer(0),
            params: ThreadStartParams {
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(config.cwd.to_string_lossy().to_string()),
                approval_policy: Some(ApiAskForApproval::Never),
                sandbox: Some(ApiSandboxMode::DangerFullAccess),
                ephemeral: Some(false),
                ..Default::default()
            },
        })
        .await
        .map_err(|e| anyhow::anyhow!("thread/start: {e}"))?;
    Ok(resp.thread.id)
}

/// Find the codex app-server binary.  In dev, it's built alongside nexal
/// in the target directory.
fn find_app_server_binary() -> anyhow::Result<String> {
    // Check if there's a compiled binary in the same target dir
    let candidates = [
        // Built from workspace
        concat!(env!("CARGO_MANIFEST_DIR"), "/../target/debug/nexal-app-server"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../target/release/nexal-app-server"),
        // System-installed
        "/usr/local/bin/nexal-app-server",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    // Try PATH
    if let Ok(output) = std::process::Command::new("which")
        .arg("nexal-app-server")
        .output()
    {
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }

    anyhow::bail!(
        "nexal-app-server binary not found. Build it with: cargo build -p nexal-app-server"
    )
}

/// Retry WebSocket connection with backoff.
async fn retry_connect(
    args: RemoteAppServerConnectArgs,
    max_attempts: u32,
) -> anyhow::Result<RemoteAppServerClient> {
    let mut last_err = None;
    for i in 0..max_attempts {
        match RemoteAppServerClient::connect(args.clone()).await {
            Ok(client) => return Ok(client),
            Err(e) => {
                last_err = Some(e);
                if i < max_attempts - 1 {
                    debug!("ws connect attempt {}/{max_attempts} failed, retrying...", i + 1);
                    tokio::time::sleep(Duration::from_millis(300 * (i as u64 + 1))).await;
                }
            }
        }
    }
    Err(last_err
        .map(|e| anyhow::anyhow!("{e}"))
        .unwrap_or_else(|| anyhow::anyhow!("connection failed")))
}
