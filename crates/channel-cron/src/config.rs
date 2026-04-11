//! Cron channel configuration.

use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};

/// Cron channel configuration.
///
/// ```toml
/// [channel.cron]
/// tick_interval_secs = 15
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct CronChannelConfig {
    /// How often to check for due jobs (seconds, default: 15).
    pub tick_interval_secs: Option<u64>,
}

impl CronChannelConfig {
    /// Extract the Cron config from the top-level `NexalConfig`.
    pub fn from_nexal_config(cfg: &NexalConfig) -> Self {
        cfg.channel
            .get("cron")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default()
    }
}
