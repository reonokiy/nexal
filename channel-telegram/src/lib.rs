//! Telegram channel adapter for nexal.
//!
//! Implements the [`Channel`] trait, routing Telegram messages through
//! the Bot orchestrator's debounce/agent pipeline.

use std::sync::Arc;

use nexal_channel_core::{Channel, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use teloxide::prelude::*;
use tracing::{info, warn};

/// Telegram channel that implements the [`Channel`] trait.
pub struct TelegramChannel {
    config: Arc<NexalConfig>,
}

impl TelegramChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let token = self
            .config
            .telegram_bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TELEGRAM_BOT_TOKEN is not set"))?
            .clone();

        info!("starting Telegram channel");
        let bot = Bot::new(token);
        let config = Arc::clone(&self.config);
        let on_message = Arc::new(on_message);

        let handler = Update::filter_message()
            .filter_map(|msg: Message| msg.text().map(str::to_string))
            .endpoint(
                move |_bot: Bot, msg: Message, text: String| {
                    let config = Arc::clone(&config);
                    let on_message = Arc::clone(&on_message);
                    async move {
                        let chat_id = msg.chat.id.0.to_string();
                        let username = msg
                            .from
                            .as_ref()
                            .and_then(|u| u.username.as_deref())
                            .unwrap_or("unknown");

                        // Access control
                        if !config.is_telegram_allowed_chat(&chat_id) {
                            warn!("rejected message from chat {chat_id}");
                            return Ok(());
                        }
                        // Skip user check for system bots (Channel_Bot forwards)
                        if username != "Channel_Bot" && !config.is_telegram_allowed_user(username) {
                            warn!("rejected message from user @{username}");
                            return Ok(());
                        }

                        info!("telegram message from @{username} in {chat_id}");

                        // Detect if bot was mentioned:
                        // - Private chat: always mentioned
                        // - Group: reply to bot, or @mention in text
                        let bot_username = _bot.get_me().await
                            .map(|me| me.username.clone().unwrap_or_default())
                            .unwrap_or_default();
                        let is_mentioned = msg.chat.is_private()
                            || msg
                                .reply_to_message()
                                .and_then(|r| r.from.as_ref())
                                .map(|u| u.is_bot)
                                .unwrap_or(false)
                            || (!bot_username.is_empty()
                                && text.contains(&format!("@{bot_username}")));

                        let incoming = IncomingMessage {
                            channel: "telegram".to_string(),
                            chat_id,
                            sender: username.to_string(),
                            text,
                            timestamp: msg.date.timestamp_millis(),
                            is_mentioned,
                            metadata: serde_json::json!({
                                "message_id": msg.id.0,
                            }),
                            images: Vec::new(),
                        };

                        on_message(incoming);
                        Ok::<(), anyhow::Error>(())
                    }
                },
            );

        Dispatcher::builder(bot, handler)
            .build()
            .dispatch()
            .await;

        Ok(())
    }

    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let token = self
            .config
            .telegram_bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TELEGRAM_BOT_TOKEN not set"))?;

        let bot = Bot::new(token);
        let chat: ChatId = ChatId(
            chat_id
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid chat_id: {chat_id}"))?,
        );

        let send_result = bot
            .send_message(chat, text)
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await;

        if send_result.is_err() {
            // Fallback: plain text if markdown parsing fails
            bot.send_message(chat, text)
                .await
                .map_err(|e| anyhow::anyhow!("telegram send failed: {e}"))?;
        }

        Ok(())
    }
}
