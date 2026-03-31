//! Persistent per-session agent pool.
//!
//! Each session gets an `AgentActor` running in the background.
//! Messages are sent via non-blocking `AgentHandle::send()`.
//! Responses arrive via the event channel.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use nexal_app_server_client::AppServerClient;
use nexal_config::NexalConfig;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::actor::{AgentActor, AgentEvent, AgentHandle, AgentMessage};
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
        let handle = self.get_or_create(session_key, &msg).await?;
        handle.send(msg).await
    }

    /// Receive the next event from any actor.
    pub async fn recv_event(&self) -> Option<AgentEvent> {
        self.event_rx.lock().await.recv().await
    }

    async fn get_or_create(
        &self,
        key: &str,
        msg: &AgentMessage,
    ) -> anyhow::Result<AgentHandle> {
        // Fast path
        {
            let map = self.sessions.lock().await;
            if let Some(h) = map.get(key) {
                return Ok(h.clone());
            }
        }

        // Slow path: create session + actor.
        // Always use in-process client — the agent core runs on the host,
        // only exec commands go to the Podman container (via NEXAL_SANDBOX).
        info!("creating new agent session for {key}");
        let actor = self.create_inprocess_actor(key, msg).await?;

        let handle = actor.spawn(self.event_tx.clone());

        let mut map = self.sessions.lock().await;
        Ok(map
            .entry(key.to_string())
            .or_insert(handle)
            .clone())
    }

    async fn create_inprocess_actor(
        &self,
        key: &str,
        msg: &AgentMessage,
    ) -> anyhow::Result<AgentActor> {
        let soul = self.build_base_instructions(msg).await;
        info!(
            session = %key,
            base_instructions_len = soul.len(),
            "prepared base instructions for in-process session"
        );
        let cli_overrides = crate::runner::providers_to_cli_overrides_full(&self.config);
        let codex_config = Arc::new(
            build_nexal_config_loader(&self.config, soul)
                .await
                .context("building config")?,
        );
        let mut client = build_client(Arc::clone(&codex_config), cli_overrides).await?;
        let thread_id = start_thread(&mut client, &codex_config).await?;
        info!("in-process session ready: thread={thread_id}");

        Ok(AgentActor::new(
            key.to_string(),
            AppServerClient::InProcess(client),
            thread_id,
            Arc::clone(&self.config),
        ))
    }

    async fn build_base_instructions(&self, msg: &AgentMessage) -> String {
        let soul = self.config.load_soul().await;
        let Some((channel_name, sender)) = session_context_from_message(msg) else {
            return soul;
        };

        let builtin_dir = self.config.workspace.join("agents").join("skills");
        let override_dir = self.config.workspace.join("agents").join("skills.override");
        let is_admin = self.config.is_admin(sender);
        let skill_docs = crate::skills::load_skill_docs(
            &builtin_dir,
            &override_dir,
            &[channel_name],
            is_admin,
        )
        .await;

        if skill_docs.trim().is_empty() || skill_docs.trim() == "(no skills available)" {
            warn!(
                channel = channel_name,
                sender = sender,
                is_admin,
                builtin_dir = %builtin_dir.display(),
                override_dir = %override_dir.display(),
                "no channel skill docs found for headless session"
            );
        } else {
            info!(
                channel = channel_name,
                sender = sender,
                is_admin,
                skill_docs_len = skill_docs.len(),
                "loaded channel skill docs for headless session"
            );
        }

        if skill_docs.trim().is_empty() || skill_docs.trim() == "(no skills available)" {
            soul
        } else {
            format!(
                "{soul}\n\n\
                 ---\n\n\
                 # Channel Response Protocol\n\n\
                 You are operating in headless mode — you have NO direct text output to the user.\n\
                 Your plain text responses are NOT delivered. The ONLY way to communicate with \
                 the user is by executing the channel skill scripts described below.\n\n\
                 For EVERY reply, you MUST call the appropriate skill script via the exec tool. \
                 Each incoming message includes a `[channel=... chat_id=...]` header — use those \
                 values as arguments to the skill script.\n\n\
                 Other channel capabilities (send files, edit messages, reactions, etc.) are also \
                 available through the skill scripts. Refer to the skill documentation below.\n\n\
                 ---\n\n\
                 {skill_docs}"
            )
        }
    }

}

fn session_context_from_message(msg: &AgentMessage) -> Option<(&str, &str)> {
    match msg {
        AgentMessage::UserInput {
            sender,
            channel,
            ..
        } => Some((channel.as_str(), sender.as_str())),
        AgentMessage::Interrupt | AgentMessage::Shutdown => None,
    }
}

