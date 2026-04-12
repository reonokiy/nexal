//! Global sandbox state — set once at startup, read everywhere.
//!
//! Nexal always runs inside a Podman container. There is no pluggable
//! backend and no "sandbox off" mode. This module stores the container
//! name (set once on startup) and exposes it for the exec layer.

use std::sync::OnceLock;

static SANDBOX: OnceLock<SandboxState> = OnceLock::new();

/// Runtime sandbox state: just the container name.
#[derive(Debug, Clone)]
pub struct SandboxState {
    /// Container name (e.g. "nexal-abc123").
    pub container: String,
}

impl SandboxState {
    /// Initialize the global sandbox state. Call once at startup.
    pub fn init(container: String) {
        let _ = SANDBOX.set(SandboxState { container });
    }

    /// Get the global sandbox state.
    pub fn get() -> &'static SandboxState {
        SANDBOX.get_or_init(|| SandboxState {
            container: "nexal-unknown".to_string(),
        })
    }

    /// Get the container name. Always available (Podman is always on).
    pub fn container_name() -> &'static str {
        &Self::get().container
    }
}
