use std::sync::Arc;

use nexal_agent::AgentPool;
use nexal_config::NexalConfig;
use nexal_state::StateDb;
use serenity::all::Context;
use serenity::all::EventHandler;
use serenity::all::GatewayIntents;
use serenity::all::Message;
use serenity::async_trait;
use serenity::Client;
use tracing::error;
use tracing::info;
use tracing::warn;

struct Handler {
    pool: Arc<AgentPool>,
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore bot messages
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

        info!("discord message from {username} in {channel_id}: {text:?}");

        let mut session = match self.db.get_or_create_session("discord", &channel_id).await {
            Ok(s) => s,
            Err(e) => {
                error!("db error: {e}");
                return;
            }
        };

        let _ = self
            .db
            .save_message(&session.id, username, "user", &text)
            .await;

        // Typing indicator while agent works
        let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

        let session_key = format!("discord:{channel_id}");
        let (messages, thread_id) = match self.pool.run_turn(&session_key, text).await {
            Ok(r) => r,
            Err(e) => {
                error!("agent error: {e}");
                (vec![format!("⚠️ Error: {e}")], String::new())
            }
        };

        if !thread_id.is_empty() {
            session.thread_id = Some(thread_id);
        }
        let _ = self.db.upsert_session(&session).await;

        for message in &messages {
            if let Err(e) = msg.channel_id.say(&ctx.http, message).await {
                error!("discord send error: {e}");
            }
            let _ = self
                .db
                .save_message(&session.id, "nexal", "assistant", message)
                .await;
        }
    }

    async fn ready(&self, _ctx: Context, ready: serenity::all::Ready) {
        info!("Discord bot connected as {}", ready.user.name);
    }
}

/// Run the Discord bot until the process is killed.
pub async fn run(
    pool: Arc<AgentPool>,
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
) -> anyhow::Result<()> {
    let token = config
        .discord_bot_token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("DISCORD_BOT_TOKEN is not set"))?
        .clone();

    info!("starting Discord bot");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(token, intents)
        .event_handler(Handler { pool, config, db })
        .await
        .map_err(|e| anyhow::anyhow!("Discord client init failed: {e}"))?;

    client
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Discord client error: {e}"))
}
