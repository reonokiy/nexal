//! Global sandbox state — set once at startup, read everywhere.
//!
//! Replaces the NEXAL_SANDBOX / NEXAL_SANDBOX_CONTAINER env vars.

use std::sync::OnceLock;

static SANDBOX: OnceLock<SandboxState> = OnceLock::new();

/// Runtime sandbox configuration.
#[derive(Debug, Clone)]
pub struct SandboxState {
    /// Container name (e.g. "nexal-abc123"). None = sandbox disabled.
    pub container: Option<String>,
}

impl SandboxState {
    /// Initialize the global sandbox state. Call once at startup.
    pub fn init(container: Option<String>) {
        let _ = SANDBOX.set(SandboxState { container });
    }

    /// Get the global sandbox state.
    pub fn get() -> &'static SandboxState {
        SANDBOX.get_or_init(|| SandboxState { container: None })
    }

    /// Is a Podman container active?
    pub fn is_active() -> bool {
        Self::get().container.is_some()
    }

    /// Get the container name, if active.
    pub fn container_name() -> Option<&'static str> {
        Self::get().container.as_deref()
    }
}
