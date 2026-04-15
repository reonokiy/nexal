//! Pluggable container backend abstraction.
//!
//! The gateway is the only thing that touches the host's container
//! runtime. Today we ship a podman implementation; future backends
//! (docker, k8s pods, gvisor, …) just have to implement
//! [`ContainerBackend`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

pub mod podman;

pub use podman::PodmanBackend;

#[derive(Debug, Clone, Error)]
pub enum BackendError {
    #[error("container backend command failed: {0}")]
    Cli(String),
    #[error("could not discover container ws port for {0}")]
    PortDiscovery(String),
    #[error("container io error: {0}")]
    Io(String),
}

/// Inputs for `ensure_container`. Filled in from the gateway's default
/// spec plus per-spawn overrides from the frontend.
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// Container name (must be unique on the host). Derived by the
    /// caller — the backend does not generate it.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// Extra env vars merged on top of the gateway's defaults.
    pub env: HashMap<String, String>,
    /// Labels to attach (gateway adds its own `app=nexal` family on top).
    pub labels: HashMap<String, String>,
    /// Optional host directory bind-mounted at `/workspace` in the container.
    pub workspace: Option<String>,
    /// Host path to the `nexal-agent` binary (copied into the container at
    /// `/usr/local/bin/nexal-agent` on first creation).
    pub agent_bin: PathBuf,
    pub memory: Option<String>,
    pub cpus: Option<String>,
    pub pids_limit: Option<u32>,
    /// Allow outbound DNS / network from inside the container.
    pub network: bool,
}

/// Result of `ensure_container`: enough for the gateway to dial the
/// in-container nexal-agent over WS.
#[derive(Debug, Clone)]
pub struct ContainerHandle {
    pub name: String,
    /// `ws://127.0.0.1:<host-port>` reachable from the gateway process.
    pub ws_url: String,
}

#[async_trait]
pub trait ContainerBackend: Send + Sync {
    /// Backend identifier (`"podman"` / `"docker"` / …).
    fn name(&self) -> &'static str;

    /// Create-or-reuse a container with the given spec. Reuse logic is
    /// keyed by `spec.name`: an existing container with the same name
    /// is started (if stopped) and returned without re-creating.
    async fn ensure(&self, spec: ContainerSpec) -> Result<ContainerHandle, BackendError>;

    /// Hard tear down (stop + remove) by container name. Idempotent.
    async fn destroy(&self, name: &str) -> Result<(), BackendError>;

    /// Best-effort: check whether a container with `name` exists on the
    /// host (any state).
    async fn exists(&self, name: &str) -> Result<bool, BackendError>;

    /// Discover the host-mapped WS URL for an existing running container.
    async fn ws_url(&self, name: &str) -> Result<String, BackendError>;
}

pub type SharedBackend = Arc<dyn ContainerBackend>;
