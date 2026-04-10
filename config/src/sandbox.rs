//! Global sandbox state — set once at startup, read everywhere.

use std::sync::OnceLock;

static SANDBOX: OnceLock<SandboxState> = OnceLock::new();

/// Runtime sandbox configuration.
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

    /// Sandbox is always active (Podman container always created).
    pub fn is_active() -> bool {
        true
    }

    /// Get the container name.
    pub fn container_name() -> Option<&'static str> {
        Some(&Self::get().container)
    }
}
