//! HTTP channel configuration.

use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};

/// HTTP channel configuration.
///
/// ```toml
/// [channel.http]
/// port = 3000
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct HttpChannelConfig {
    pub port: Option<u16>,
}

impl HttpChannelConfig {
    /// Extract the HTTP config from the top-level `NexalConfig`.
    pub fn from_nexal_config(cfg: &NexalConfig) -> Self {
        let mut this: Self = cfg
            .channel
            .get("http")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        // Backward-compat: flat `http_channel_port` field.
        if this.port.is_none() {
            this.port = cfg.http_channel_port;
        }

        this
    }
}
