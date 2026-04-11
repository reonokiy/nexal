//! Heartbeat channel configuration.

use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};

/// Heartbeat channel configuration.
///
/// ```toml
/// [channel.heartbeat]
/// interval_mins = 30
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct HeartbeatChannelConfig {
    /// Interval in minutes between heartbeats (default: 30).
    pub interval_mins: Option<u64>,
}

impl HeartbeatChannelConfig {
    /// Extract the Heartbeat config from the top-level `NexalConfig`.
    pub fn from_nexal_config(cfg: &NexalConfig) -> Self {
        let mut this: Self = cfg
            .channel
            .get("heartbeat")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        // Backward-compat: flat `heartbeat_interval_mins` field.
        if this.interval_mins.is_none() {
            this.interval_mins = cfg.heartbeat_interval_mins;
        }

        this
    }
}
