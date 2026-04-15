//! AgentRegistry — owns container lifecycle + the WS connection to
//! each running agent. The frontend talks to one of these per gateway
//! instance.
//!
//! The registry keeps an in-memory `HashMap<AgentId, AgentEntry>`. An
//! agentId is a freshly minted UUID for every spawn. On `attach` (used
//! when the frontend re-connects after a restart) the registry creates
//! a NEW agentId for the same container — frontend is responsible for
//! mapping its own logical worker id ↔ container_name across restarts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, Mutex};
use uuid::Uuid;

use crate::agent_conn::{AgentConn, AgentConnError, AgentNotification};
use crate::backend::{BackendError, ContainerHandle, ContainerSpec, SharedBackend};
use crate::proxy::SharedProxyRegistry;

pub type AgentId = String;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("backend error: {0}")]
    Backend(#[from] BackendError),
    #[error("agent connect error: {0}")]
    AgentConn(#[from] AgentConnError),
    #[error("agent {0} not found")]
    UnknownAgent(String),
    #[error("container {0} not found")]
    UnknownContainer(String),
}

#[derive(Clone)]
pub struct AgentEntry {
    pub agent_id: AgentId,
    pub container_name: String,
    pub created_at_unix_ms: u64,
    pub conn: Arc<AgentConn>,
}

pub struct AgentRegistry {
    backend: SharedBackend,
    /// Defaults applied to every spawn unless overridden.
    pub spawn_defaults: SpawnDefaults,
    inner: Mutex<HashMap<AgentId, AgentEntry>>,
    /// Notifications coming from any agent are broadcast to every
    /// frontend session subscribed via `subscribe_notifications`.
    notify_tx: broadcast::Sender<TaggedNotification>,
    /// Shared proxy registry — the agent registry owns lifecycle
    /// cleanup (when an agent is killed, all its proxies are dropped).
    pub proxies: SharedProxyRegistry,
}

#[derive(Clone)]
pub struct SpawnDefaults {
    pub image: String,
    pub agent_bin: PathBuf,
    pub workspace: Option<String>,
    pub memory: Option<String>,
    pub cpus: Option<String>,
    pub pids_limit: Option<u32>,
    pub network: bool,
    pub container_name_prefix: String,
}

#[derive(Debug, Clone)]
pub struct TaggedNotification {
    pub agent_id: AgentId,
    pub method: String,
    pub params: Option<Value>,
}

impl AgentRegistry {
    pub fn new(
        backend: SharedBackend,
        spawn_defaults: SpawnDefaults,
        proxies: SharedProxyRegistry,
    ) -> Self {
        let (notify_tx, _) = broadcast::channel(256);
        Self {
            backend,
            spawn_defaults,
            inner: Mutex::new(HashMap::new()),
            notify_tx,
            proxies,
        }
    }

    /// Subscribe to the broadcast stream of agent notifications. Each
    /// frontend session calls this once.
    pub fn subscribe_notifications(&self) -> broadcast::Receiver<TaggedNotification> {
        self.notify_tx.subscribe()
    }

    /// Create a new container + open its agent connection. Returns the
    /// fresh agentId.
    pub async fn spawn(
        &self,
        name: String,
        image: Option<String>,
        env: HashMap<String, String>,
        labels: HashMap<String, String>,
        workspace: Option<String>,
    ) -> Result<AgentEntry, RegistryError> {
        let container_name = self.derive_container_name(&name);
        let spec = ContainerSpec {
            name: container_name.clone(),
            image: image.unwrap_or_else(|| self.spawn_defaults.image.clone()),
            env,
            labels,
            workspace: workspace.or_else(|| self.spawn_defaults.workspace.clone()),
            agent_bin: self.spawn_defaults.agent_bin.clone(),
            memory: self.spawn_defaults.memory.clone(),
            cpus: self.spawn_defaults.cpus.clone(),
            pids_limit: self.spawn_defaults.pids_limit,
            network: self.spawn_defaults.network,
        };
        let handle = self.backend.ensure(spec).await?;
        let entry = self.dial_and_register(handle).await?;
        Ok(entry)
    }

    /// Re-attach to an existing container (no-op if not running). Used
    /// by the frontend after a restart to recover live workers.
    pub async fn attach(&self, container_name: String) -> Result<AgentEntry, RegistryError> {
        if !self.backend.exists(&container_name).await? {
            return Err(RegistryError::UnknownContainer(container_name));
        }
        // Make sure it's started, then discover the WS URL.
        let _ = self.backend.ensure(ContainerSpec {
            name: container_name.clone(),
            image: self.spawn_defaults.image.clone(), // unused on the reuse path
            env: Default::default(),
            labels: Default::default(),
            workspace: self.spawn_defaults.workspace.clone(),
            agent_bin: self.spawn_defaults.agent_bin.clone(),
            memory: self.spawn_defaults.memory.clone(),
            cpus: self.spawn_defaults.cpus.clone(),
            pids_limit: self.spawn_defaults.pids_limit,
            network: self.spawn_defaults.network,
        }).await?;
        let ws_url = self.backend.ws_url(&container_name).await?;
        let handle = ContainerHandle {
            name: container_name,
            ws_url,
        };
        self.dial_and_register(handle).await
    }

    async fn dial_and_register(
        &self,
        handle: ContainerHandle,
    ) -> Result<AgentEntry, RegistryError> {
        let agent_id = Uuid::new_v4().to_string();

        // Bridge per-agent notifications onto the gateway-wide tagged channel.
        let (per_agent_tx, mut per_agent_rx) = mpsc::channel::<AgentNotification>(64);
        let agent_id_for_bridge = agent_id.clone();
        let global_tx = self.notify_tx.clone();
        tokio::spawn(async move {
            while let Some(notif) = per_agent_rx.recv().await {
                // broadcast::send fails only if no subscribers; that's
                // fine — drop notifications when no frontend is listening.
                let _ = global_tx.send(TaggedNotification {
                    agent_id: agent_id_for_bridge.clone(),
                    method: notif.method,
                    params: notif.params,
                });
            }
        });

        let conn = AgentConn::connect(&handle.ws_url, &format!("nexal-gateway/{agent_id}"), per_agent_tx)
            .await?;
        let entry = AgentEntry {
            agent_id: agent_id.clone(),
            container_name: handle.name,
            created_at_unix_ms: now_ms(),
            conn: Arc::new(conn),
        };
        self.inner.lock().await.insert(agent_id, entry.clone());
        Ok(entry)
    }

    /// Drop the in-memory mapping but leave the container alive. Used
    /// on graceful shutdown so the next gateway run can re-attach.
    pub async fn detach(&self, agent_id: &str) -> Result<(), RegistryError> {
        let mut map = self.inner.lock().await;
        match map.remove(agent_id) {
            Some(entry) => {
                entry.conn.close().await;
                Ok(())
            }
            None => Err(RegistryError::UnknownAgent(agent_id.into())),
        }
    }

    /// Stop + remove the container. Also cleans up any proxies
    /// registered for this agent.
    pub async fn kill(&self, agent_id: &str) -> Result<(), RegistryError> {
        let entry = {
            let mut map = self.inner.lock().await;
            map.remove(agent_id)
        };
        // Proxy cleanup runs even if the agent_id isn't in our map
        // (idempotent, and lets a dangling registration get cleaned).
        let dropped = self.proxies.cleanup_for_agent(agent_id).await;
        if dropped > 0 {
            tracing::debug!("kill {agent_id}: dropped {dropped} proxy registration(s)");
        }
        match entry {
            Some(entry) => {
                entry.conn.close().await;
                self.backend.destroy(&entry.container_name).await?;
                Ok(())
            }
            None => Err(RegistryError::UnknownAgent(agent_id.into())),
        }
    }

    pub async fn get(&self, agent_id: &str) -> Option<AgentEntry> {
        self.inner.lock().await.get(agent_id).cloned()
    }

    pub async fn list(&self) -> Vec<AgentEntry> {
        self.inner.lock().await.values().cloned().collect()
    }

    /// Tear everything down (called on gateway shutdown). Containers
    /// survive — only the in-memory mapping + WS streams are cleared.
    pub async fn detach_all(&self) {
        let entries: Vec<AgentEntry> = {
            let mut map = self.inner.lock().await;
            map.drain().map(|(_, v)| v).collect()
        };
        for entry in entries {
            entry.conn.close().await;
        }
    }

    fn derive_container_name(&self, name: &str) -> String {
        // Sanitize: only letters, digits, _ . - allowed by podman.
        let mut out = String::with_capacity(name.len() + self.spawn_defaults.container_name_prefix.len());
        out.push_str(&self.spawn_defaults.container_name_prefix);
        for c in name.chars() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                out.push(c);
            } else {
                out.push('_');
            }
        }
        out
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
