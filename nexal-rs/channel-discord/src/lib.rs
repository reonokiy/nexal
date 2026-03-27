//! Discord channel adapter for nexal.
//!
//! Implements the [`Channel`] trait, routing Discord messages through
//! the Bot orchestrator's debounce/agent pipeline.

use std::sync::Arc;

use nexal_channel_core::{Channel, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use serenity::all::{Context, EventHandler, GatewayIntents, Message, Ready};
use serenity::Client;
use tracing::{info, warn};

/// Discord channel that implements the [`Channel`] trait.
pub struct DiscordChannel {
    config: Arc<NexalConfig>,
}

impl DiscordChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let token = self
            .config
            .discord_bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DISCORD_BOT_TOKEN is not set"))?
            .clone();

        info!("starting Discord channel");

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let handler = DiscordHandler {
            config: Arc::clone(&self.config),
            on_message: Arc::new(on_message),
        };

        let mut client = Client::builder(token, intents)
            .event_handler(handler)
            .await
            .map_err(|e| anyhow::anyhow!("Discord client init failed: {e}"))?;

        client
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("Discord client error: {e}"))
    }

    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        // Note: Discord send requires an active client context.
        // For the Bot orchestrator, responses are sent by the agent via
        // skill scripts (exec) rather than this method.
        // This is a best-effort fallback using the REST API directly.
        let token = self
            .config
            .discord_bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DISCORD_BOT_TOKEN not set"))?;

        let http = serenity::http::Http::new(token);
        let channel_id: u64 = chat_id
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid channel_id: {chat_id}"))?;

        serenity::all::ChannelId::new(channel_id)
            .say(&http, text)
            .await
            .map_err(|e| anyhow::anyhow!("discord send failed: {e}"))?;

        Ok(())
    }
}

struct DiscordHandler {
    config: Arc<NexalConfig>,
    on_message: Arc<MessageCallback>,
}

#[async_trait::async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let text = msg.content.trim().to_string();
        if text.is_empty() {
            return;
        }

        let guild_id = msg.guild_id.map(|g| g.to_string()).unwrap_or_default();
        let channel_id = msg.channel_id.to_string();
        let username = msg.author.name.as_str();

        // Access control
        if !guild_id.is_empty() && !self.config.is_discord_allowed_guild(&guild_id) {
            warn!("rejected message from guild {guild_id}");
            return;
        }

        info!("discord message from {username} in {channel_id}");

        // In DMs, always mentioned. In guilds, check for bot mention.
        let is_mentioned = guild_id.is_empty() || msg.mentions_me(&_ctx).await.unwrap_or(false);

        let incoming = IncomingMessage {
            channel: "discord".to_string(),
            chat_id: channel_id,
            sender: username.to_string(),
            text,
            timestamp: msg.timestamp.unix_timestamp() * 1000,
            is_mentioned,
            metadata: serde_json::json!({
                "message_id": msg.id.get(),
                "guild_id": guild_id,
            }),
            images: Vec::new(),
        };

        (self.on_message)(incoming);
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);
    }
}
