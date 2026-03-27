use std::sync::Arc;

use nexal_agent::AgentPool;
use nexal_config::NexalConfig;
use nexal_state::StateDb;
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Run the Telegram bot until the process is killed.
pub async fn run(
    pool: Arc<AgentPool>,
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
) -> anyhow::Result<()> {
    let token = config
        .telegram_bot_token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("TELEGRAM_BOT_TOKEN is not set"))?
        .clone();

    info!("starting Telegram bot");
    let bot = Bot::new(token);

    Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![pool, config, db])
        .build()
        .dispatch()
        .await;

    Ok(())
}

fn schema() -> UpdateHandler<anyhow::Error> {
    Update::filter_message()
        .filter_map(|msg: Message| msg.text().map(str::to_string))
        .endpoint(handle_message)
}

async fn handle_message(
    bot: Bot,
    msg: Message,
    text: String,
    pool: Arc<AgentPool>,
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
) -> anyhow::Result<()> {
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
    if !config.is_telegram_allowed_user(username) {
        warn!("rejected message from user @{username}");
        return Ok(());
    }

    info!("telegram message from @{username} in {chat_id}: {text:?}");

    // Load or create session record for DB tracking
    let mut session = db
        .get_or_create_session("telegram", &chat_id)
        .await
        .map_err(|e| {
            error!("db error: {e}");
            e
        })?;

    let _ = db
        .save_message(&session.id, username, "user", &text)
        .await;

    let session_key = format!("telegram:{chat_id}");
    let (messages, thread_id) = match pool.run_turn(&session_key, text).await {
        Ok(r) => r,
        Err(e) => {
            error!("agent error: {e}");
            (vec![format!("⚠️ Error: {e}")], String::new())
        }
    };

    if !thread_id.is_empty() {
        session.thread_id = Some(thread_id);
    }
    let _ = db.upsert_session(&session).await;

    for message in &messages {
        let send_result = bot
            .send_message(msg.chat.id, message.clone())
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .await;
        if send_result.is_err() {
            // Fallback: plain text if markdown parsing fails
            bot.send_message(msg.chat.id, message)
                .await
                .map_err(|e| anyhow::anyhow!("send_message failed: {e}"))?;
        }

        let _ = db
            .save_message(&session.id, "nexal", "assistant", message)
            .await;
    }

    Ok(())
}
