//! Discord channel configuration.

use nexal_channel_core::serde_utils::deserialize_string_or_int_vec;
use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};

/// Discord channel configuration.
///
/// ```toml
/// [channel.discord]
/// bot_token = "MTIz..."
/// allow_guilds = ["123456789"]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DiscordChannelConfig {
    pub bot_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub allow_guilds: Vec<String>,
}

impl DiscordChannelConfig {
    /// Extract the Discord config from the top-level `NexalConfig`.
    pub fn from_nexal_config(cfg: &NexalConfig) -> Self {
        let mut this: Self = cfg
            .channel
            .get("discord")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        // Backward-compat: flat `discord_bot_token` field.
        if this.bot_token.is_none() {
            if let Some(ref token) = cfg.discord_bot_token {
                this.bot_token = Some(token.clone());
            }
        }

        // Normalize comma-separated list fields.
        fn normalize(v: Vec<String>) -> Vec<String> {
            v.into_iter()
                .flat_map(|s| {
                    s.split(',')
                        .map(|a| a.trim().trim_matches('@').to_string())
                        .collect::<Vec<_>>()
                })
                .filter(|a| !a.is_empty())
                .collect()
        }
        this.allow_guilds = normalize(this.allow_guilds);

        this
    }

    pub fn is_allowed_guild(&self, guild_id: &str) -> bool {
        self.allow_guilds.is_empty()
            || self.allow_guilds.iter().any(|g| g == guild_id)
    }
}
