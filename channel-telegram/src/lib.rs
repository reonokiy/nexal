//! Telegram channel adapter for nexal.
//!
//! Implements the [`Channel`] trait, routing Telegram messages through
//! the Bot orchestrator's debounce/agent pipeline.
//! Handles text, photos, documents, stickers, and captions.

use std::sync::Arc;

use nexal_channel_core::{Channel, ImageAttachment, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{MediaKind, MessageKind};
use tracing::{debug, info};

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

        // Accept ALL messages, not just text
        let handler = Update::filter_message().endpoint(
            move |the_bot: Bot, msg: Message| {
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
                        return Ok(());
                    }
                    if username != "Channel_Bot"
                        && !config.is_telegram_allowed_user(username)
                    {
                        return Ok(());
                    }

                    // Extract text + media info from message
                    let (text, images) = extract_message_content(&the_bot, &msg).await;

                    // Skip completely empty messages (e.g. service messages)
                    if text.is_empty() && images.is_empty() {
                        return Ok(());
                    }

                    let full_text = text;

                    info!(
                        "telegram message from @{username} in {chat_id}: {}",
                        if full_text.len() > 50 {
                            format!("{}...", &full_text[..50])
                        } else {
                            full_text.clone()
                        }
                    );

                    // Detect mention
                    let bot_username = the_bot
                        .get_me()
                        .await
                        .map(|me| me.username.clone().unwrap_or_default())
                        .unwrap_or_default();
                    let is_mentioned = msg.chat.is_private()
                        || msg
                            .reply_to_message()
                            .and_then(|r| r.from.as_ref())
                            .map(|u| u.is_bot)
                            .unwrap_or(false)
                        || (!bot_username.is_empty()
                            && full_text.contains(&format!("@{bot_username}")));

                    let is_admin = config.is_admin(username);
                    let incoming = IncomingMessage {
                        channel: "telegram".to_string(),
                        chat_id,
                        sender: username.to_string(),
                        text: full_text,
                        timestamp: msg.date.timestamp_millis(),
                        is_mentioned,
                        metadata: serde_json::json!({
                            "message_id": msg.id.0,
                            "is_admin": is_admin,
                        }),
                        images,
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
            bot.send_message(chat, text)
                .await
                .map_err(|e| anyhow::anyhow!("telegram send failed: {e}"))?;
        }

        Ok(())
    }
}


/// Extract text and images from a Telegram message.
async fn extract_message_content(
    bot: &Bot,
    msg: &Message,
) -> (String, Vec<ImageAttachment>) {
    let MessageKind::Common(common) = &msg.kind else {
        return (String::new(), Vec::new());
    };

    match &common.media_kind {
        MediaKind::Text(text_media) => {
            (text_media.text.clone(), Vec::new())
        }

        MediaKind::Photo(photo) => {
            let caption = photo.caption.clone().unwrap_or_default();
            let mut images = Vec::new();

            // Download the largest photo
            if let Some(largest) = photo.photo.last() {
                if let Ok(data) = download_file(bot, &largest.file.id).await {
                    images.push(ImageAttachment {
                        data,
                        mime_type: "image/jpeg".to_string(),
                        filename: format!("{}.jpg", largest.file.unique_id),
                    });
                }
            }

            (caption, images)
        }

        MediaKind::Document(doc) => {
            let caption = doc.caption.clone().unwrap_or_default();
            let name = doc.document.file_name.as_deref().unwrap_or("file");
            let mime = doc.document.mime_type.as_ref().map(|m| m.to_string()).unwrap_or_default();
            let text = format!(
                "{caption}\n[received document: {name} ({mime}), file_id: {}]",
                doc.document.file.id
            ).trim().to_string();
            (text, Vec::new())
        }

        MediaKind::Sticker(sticker) => {
            let emoji = sticker.sticker.emoji.clone().unwrap_or_default();
            let mut images = Vec::new();
            // Download thumbnail as image
            if let Some(thumb) = &sticker.sticker.thumbnail {
                if let Ok(data) = download_file(bot, &thumb.file.id).await {
                    images.push(ImageAttachment {
                        data,
                        mime_type: "image/jpeg".to_string(),
                        filename: format!("{}.jpg", thumb.file.unique_id),
                    });
                }
            }
            let text = format!("[sticker {emoji}, file_id: {}]", sticker.sticker.file.id);
            (text, images)
        }

        MediaKind::Voice(voice) => {
            let caption = voice.caption.clone().unwrap_or_default();
            let text = format!(
                "{caption}\n[received voice message, file_id: {}]",
                voice.voice.file.id
            ).trim().to_string();
            (text, Vec::new())
        }

        MediaKind::Video(video) => {
            let caption = video.caption.clone().unwrap_or_default();
            let name = video.video.file_name.as_deref().unwrap_or("video");
            let text = format!(
                "{caption}\n[received video: {name}, file_id: {}]",
                video.video.file.id
            ).trim().to_string();
            (text, Vec::new())
        }

        MediaKind::Animation(anim) => {
            let caption = anim.caption.clone().unwrap_or_default();
            let text = format!(
                "{caption}\n[received animation/gif, file_id: {}]",
                anim.animation.file.id
            ).trim().to_string();
            (text, Vec::new())
        }

        MediaKind::Audio(audio) => {
            let caption = audio.caption.clone().unwrap_or_default();
            let title = audio.audio.title.as_deref().unwrap_or("audio");
            let text = format!(
                "{caption}\n[received audio: {title}, file_id: {}]",
                audio.audio.file.id
            ).trim().to_string();
            (text, Vec::new())
        }

        _ => {
            debug!("unhandled media kind in telegram message");
            (String::new(), Vec::new())
        }
    }
}

/// Download a file from Telegram by file_id.
async fn download_file(bot: &Bot, file_id: &str) -> Result<Vec<u8>, teloxide::RequestError> {
    let file = bot.get_file(file_id).await?;
    let mut buf = Vec::new();
    bot.download_file(&file.path, &mut buf).await?;
    Ok(buf)
}
