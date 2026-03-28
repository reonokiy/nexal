//! Persistent per-session agent pool.
//!
//! Each session gets an `AgentActor` running in the background.
//! Messages are sent via non-blocking `AgentHandle::send()`.
//! Responses arrive via the event channel.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use nexal_app_server_client::AppServerClient;
use nexal_app_server_client::RemoteAppServerConnectArgs;
use nexal_app_server_client::RemoteAppServerClient;
use nexal_app_server_protocol::AskForApproval as ApiAskForApproval;
use nexal_app_server_protocol::ClientRequest;
use nexal_app_server_protocol::RequestId;
use nexal_app_server_protocol::SandboxMode as ApiSandboxMode;
use nexal_app_server_protocol::ThreadStartParams;
use nexal_app_server_protocol::ThreadStartResponse;
use nexal_config::NexalConfig;
use nexal_config::SandboxBackend;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::actor::{AgentActor, AgentEvent, AgentHandle, AgentMessage};
use crate::podman::PodmanContainer;
use crate::runner::{
    build_client, build_nexal_config_loader, start_thread,
};

/// A pool of persistent agent sessions, one per chat.
///
/// Thread-safe; concurrent messages on *different* sessions run in parallel.
pub struct AgentPool {
    config: Arc<NexalConfig>,
    sessions: Mutex<HashMap<String, AgentHandle>>,
    /// Channel for receiving events from all actors.
    event_tx: mpsc::Sender<AgentEvent>,
    event_rx: Mutex<mpsc::Receiver<AgentEvent>>,
}

impl AgentPool {
    pub fn new(config: Arc<NexalConfig>) -> Arc<Self> {
        info!(
            "agent pool using sandbox backend: {}",
            config.sandbox_backend()
        );
        let (event_tx, event_rx) = mpsc::channel(256);
        Arc::new(Self {
            config,
            sessions: Mutex::new(HashMap::new()),
            event_tx,
            event_rx: Mutex::new(event_rx),
        })
    }

    /// Send a message to the agent for this session (non-blocking).
    pub async fn send(
        &self,
        session_key: &str,
        msg: AgentMessage,
    ) -> anyhow::Result<()> {
        let handle = self.get_or_create(session_key).await?;
        handle.send(msg).await
    }

    /// Receive the next event from any actor.
    pub async fn recv_event(&self) -> Option<AgentEvent> {
        self.event_rx.lock().await.recv().await
    }

    /// Convenience: blocking run_turn (for backward compatibility).
    /// Sends input, waits for the Response event, returns chunks.
    pub async fn run_turn(
        &self,
        session_key: &str,
        text: String,
    ) -> anyhow::Result<(Vec<String>, String)> {
        self.send(
            session_key,
            AgentMessage::UserInput {
                text,
                sender: String::new(),
                channel: String::new(),
            },
        )
        .await?;

        // Wait for the response event for this session.
        // This is a simple poll — in production, callers should use recv_event().
        let mut rx = self.event_rx.lock().await;
        loop {
            match rx.recv().await {
                Some(AgentEvent::Response {
                    session_key: key,
                    chunks,
                    thread_id,
                }) if key == session_key => {
                    return Ok((chunks, thread_id));
                }
                Some(AgentEvent::Error {
                    session_key: key,
                    message,
                }) if key == session_key => {
                    return Err(anyhow::anyhow!("{message}"));
                }
                Some(_) => {
                    // Event for a different session — ignore (will be lost).
                    // This is why callers should use the non-blocking API.
                    continue;
                }
                None => {
                    return Err(anyhow::anyhow!("event channel closed"));
                }
            }
        }
    }

    async fn get_or_create(
        &self,
        key: &str,
    ) -> anyhow::Result<AgentHandle> {
        // Fast path
        {
            let map = self.sessions.lock().await;
            if let Some(h) = map.get(key) {
                return Ok(h.clone());
            }
        }

        // Slow path: create session + actor
        info!("creating new agent session for {key}");
        let actor = match self.config.sandbox_backend() {
            SandboxBackend::Podman => self.create_podman_actor(key).await?,
            SandboxBackend::None => self.create_inprocess_actor(key).await?,
        };

        let handle = actor.spawn(self.event_tx.clone());

        let mut map = self.sessions.lock().await;
        Ok(map
            .entry(key.to_string())
            .or_insert(handle)
            .clone())
    }

    async fn create_inprocess_actor(&self, key: &str) -> anyhow::Result<AgentActor> {
        let soul = self.config.load_soul().await;
        let codex_config = Arc::new(
            build_nexal_config_loader(&self.config, soul)
                .await
                .context("building config")?,
        );
        let mut client = build_client(Arc::clone(&codex_config)).await?;
        let thread_id = start_thread(&mut client, &codex_config).await?;
        info!("in-process session ready: thread={thread_id}");

        Ok(AgentActor::new(
            key.to_string(),
            AppServerClient::InProcess(client),
            thread_id,
            Arc::clone(&self.config),
            None,
        ))
    }

    async fn create_podman_actor(&self, key: &str) -> anyhow::Result<AgentActor> {
        let app_server_bin = find_app_server_binary()?;
        let container = PodmanContainer::start(key, &self.config, &app_server_bin).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

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

        let client = retry_connect(connect_args, 10)
            .await
            .context("connecting to podman app-server")?;

        let soul = self.config.load_soul().await;
        let codex_config = Arc::new(
            build_nexal_config_loader(&self.config, soul)
                .await
                .context("building config for thread")?,
        );

        let thread_id = start_remote_thread(&client, &codex_config).await?;
        info!(
            "podman session ready: key={key} container={} thread={thread_id}",
            container.name
        );

        Ok(AgentActor::new(
            key.to_string(),
            AppServerClient::Remote(client),
            thread_id,
            Arc::clone(&self.config),
            Some(container),
        ))
    }
}

/// Start a thread on a remote app-server client.
async fn start_remote_thread(
    client: &RemoteAppServerClient,
    config: &nexal_core::config::Config,
) -> anyhow::Result<String> {
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

fn find_app_server_binary() -> anyhow::Result<String> {
    let candidates = [
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../target/debug/nexal-app-server"
        ),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../target/release/nexal-app-server"
        ),
        "/usr/local/bin/nexal-app-server",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

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
                    debug!(
                        "ws connect attempt {}/{max_attempts} failed, retrying...",
                        i + 1
                    );
                    tokio::time::sleep(Duration::from_millis(300 * (i as u64 + 1))).await;
                }
            }
        }
    }
    Err(last_err
        .map(|e| anyhow::anyhow!("{e}"))
        .unwrap_or_else(|| anyhow::anyhow!("connection failed")))
}
