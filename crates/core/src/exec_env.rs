use crate::config::types::ShellEnvironmentPolicy;
use nexal_protocol::ThreadId;
use std::collections::HashMap;

pub const NEXAL_THREAD_ID_ENV_VAR: &str = "NEXAL_THREAD_ID";

/// Construct an environment map for commands executed inside a container.
///
/// Does NOT inherit from the host process — commands execute inside the
/// exec-server container which has its own environment (HOME, PATH, etc.)
/// set by the container runtime. We only pass through user-configured
/// `set` overrides and the thread ID.
pub fn create_env(
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<ThreadId>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    for (key, val) in &policy.r#set {
        env.insert(key.clone(), val.clone());
    }

    if let Some(thread_id) = thread_id {
        env.insert(NEXAL_THREAD_ID_ENV_VAR.to_string(), thread_id.to_string());
    }

    env
}

#[cfg(test)]
#[path = "exec_env_tests.rs"]
mod tests;
