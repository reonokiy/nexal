//! Telegram channel configuration.

use nexal_channel_core::serde_utils::deserialize_string_or_int_vec;
use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};

/// Telegram channel configuration.
///
/// ```toml
/// [channel.telegram]
/// bot_token = "123456:ABC-DEF"
/// allow_from = ["alice", "bob"]
/// allow_chats = [-100123456]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct TelegramChannelConfig {
    pub bot_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub allow_from: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub allow_chats: Vec<String>,
}

impl TelegramChannelConfig {
    /// Extract the Telegram config from the top-level `NexalConfig`.
    ///
    /// Falls back to `TELEGRAM_BOT_TOKEN` env-var compat via the stored flat
    /// field `nexal_config.telegram_bot_token`. The `allow_from` / `allow_chats`
    /// lists are already normalized by the custom deserializer.
    pub fn from_nexal_config(cfg: &NexalConfig) -> Self {
        let mut this: Self = cfg
            .channel
            .get("telegram")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        // Backward-compat: flat `telegram_bot_token` field wins if channel
        // section is missing it.
        if this.bot_token.is_none() {
            if let Some(ref token) = cfg.telegram_bot_token {
                this.bot_token = Some(token.clone());
            }
        }

        this
    }

    pub fn is_allowed_user(&self, username: &str) -> bool {
        self.allow_from.is_empty()
            || self.allow_from.iter().any(|u| u == username)
    }

    pub fn is_allowed_chat(&self, chat_id: &str) -> bool {
        self.allow_chats.is_empty()
            || self.allow_chats.iter().any(|c| c == chat_id)
    }
}
