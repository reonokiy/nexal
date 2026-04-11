//! Telegram channel adapter for nexal.
//!
//! Implements the [`Channel`] trait, routing Telegram messages through
//! the Bot orchestrator's debounce/agent pipeline.
//! Handles text, photos, documents, stickers, and captions.

pub mod config;

use std::collections::HashMap;
use std::sync::Arc;

use config::TelegramChannelConfig;
use nexal_channel_core::{Channel, ImageAttachment, IncomingMessage, MessageCallback, TypingHandle};
use nexal_config::NexalConfig;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, MediaKind, MessageKind};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

/// Pending media group being accumulated.
struct PendingMediaGroup {
    messages: Vec<(String, Vec<ImageAttachment>, Message)>,
    #[allow(dead_code)] // Held to keep the timer task alive.
    timer: tokio::task::JoinHandle<()>,
}

/// Telegram channel that implements the [`Channel`] trait.
pub struct TelegramChannel {
    config: Arc<NexalConfig>,
    ch_config: TelegramChannelConfig,
}

impl TelegramChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        let ch_config = TelegramChannelConfig::from_nexal_config(&config);
        Self { config, ch_config }
    }
}

#[async_trait::async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let token = self
            .ch_config
            .bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TELEGRAM_BOT_TOKEN is not set"))?
            .clone();

        info!("starting Telegram channel");
        let bot = Bot::new(token);
        let config = Arc::clone(&self.config);
        let ch_config = Arc::new(self.ch_config.clone());
        let on_message = Arc::new(on_message);

        // Buffer for accumulating media group messages before dispatching.
        let media_groups: Arc<Mutex<HashMap<String, PendingMediaGroup>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Accept ALL messages, not just text
        let handler = Update::filter_message().endpoint(
            move |the_bot: Bot, msg: Message| {
                let config = Arc::clone(&config);
                let ch_cfg = Arc::clone(&ch_config);
                let on_message = Arc::clone(&on_message);
                let media_groups = Arc::clone(&media_groups);
                async move {
                    let chat_id = msg.chat.id.0.to_string();

                    // Extract the real sender. For channel-forwarded messages,
                    // msg.from is "Channel_Bot" — use author_signature, sender_chat,
                    // or forward_from for the actual identity.
                    let (username, user_id) = extract_sender(&msg);

                    // Allow if chat OR user is in the allow list.
                    // Empty list = allow all.
                    let chat_ok = ch_cfg.is_allowed_chat(&chat_id);
                    let is_channel_forward = msg.from.as_ref()
                        .and_then(|u| u.username.as_deref())
                        .is_some_and(|u| u == "Channel_Bot" || u == "GroupAnonymousBot");
                    let user_ok = is_channel_forward
                        || ch_cfg.is_allowed_user(&username);

                    if !chat_ok && !user_ok {
                        let _ = the_bot
                            .send_message(
                                msg.chat.id,
                                format!(
                                    "⚠️ Not authorized.\n\
                                    chat_id: {chat_id}\n\
                                    user: @{username} (id: {user_id})"
                                ),
                            )
                            .await;
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
                            let end = full_text.char_indices()
                                .map(|(i, _)| i)
                                .find(|&i| i >= 50)
                                .unwrap_or(full_text.len());
                            format!("{}...", &full_text[..end])
                        } else {
                            full_text.clone()
                        }
                    );

                    // If this message is part of a media group (album), buffer it
                    // and wait for the rest before dispatching.
                    if let Some(group_id) = msg.media_group_id() {
                        let group_id = group_id.to_string();
                        let mut groups = media_groups.lock().await;
                        if let Some(pending) = groups.get_mut(&group_id) {
                            // Add to existing group.
                            pending.messages.push((full_text, images, msg));
                            return Ok(());
                        }
                        // First message in this group — start a timer.
                        let mg = Arc::clone(&media_groups);
                        let gid = group_id.clone();
                        let config2 = Arc::clone(&config);
                        let on_msg = Arc::clone(&on_message);
                        let bot2 = the_bot.clone();
                        let timer = tokio::spawn(async move {
                            // Telegram sends all media group messages within ~1s.
                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                            let pending = {
                                let mut groups = mg.lock().await;
                                groups.remove(&gid)
                            };
                            if let Some(pending) = pending {
                                dispatch_media_group(
                                    pending.messages, &config2, &on_msg, &bot2,
                                ).await;
                            }
                        });
                        groups.insert(group_id, PendingMediaGroup {
                            messages: vec![(full_text, images, msg)],
                            timer,
                        });
                        return Ok(());
                    }

                    // Single message (not a media group) — dispatch immediately.
                    let is_mentioned = detect_mention(&the_bot, &msg, &full_text).await;
                    let is_admin = config.is_admin(&username);
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

    fn start_typing(&self, chat_id: &str) -> Option<TypingHandle> {
        let token = self.ch_config.bot_token.as_ref()?.clone();
        let chat: ChatId = ChatId(chat_id.parse().ok()?);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        // Telegram typing indicator expires after 5s; resend every 4s.
        tokio::spawn(async move {
            let bot = Bot::new(token);
            loop {
                let _ = bot.send_chat_action(chat, ChatAction::Typing).await;
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(4)) => {}
                    _ = cancel_clone.cancelled() => break,
                }
            }
        });

        Some(TypingHandle::new(cancel))
    }

    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let token = self
            .ch_config
            .bot_token
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
            let emoji = sticker.sticker.emoji.as_deref().unwrap_or_default();
            let set_name = sticker.sticker.set_name.as_deref().unwrap_or_default();
            // Use the thumbnail (~128x128 JPEG) for model context — enough
            // to understand the sticker without wasting tokens on a full 512x512.
            // Fall back to the full sticker file only if no thumbnail exists.
            let download_target = sticker_download_target(&sticker.sticker);
            let mut images = Vec::new();
            if let Some((file_id, unique_id, mime, ext)) = download_target {
                if let Ok(data) = download_file(bot, file_id).await {
                    images.push(ImageAttachment {
                        data,
                        mime_type: mime.to_string(),
                        filename: format!("{unique_id}.{ext}"),
                    });
                }
            }
            let set_info = (!set_name.is_empty())
                .then(|| format!(", set: {set_name}"))
                .unwrap_or_default();
            let text = format!(
                "[sticker {emoji}, file_id: {}{set_info}]",
                sticker.sticker.file.id
            );
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

/// Extract the real sender from a Telegram message.
///
/// For regular messages, `msg.from` has the correct user. But for messages
/// forwarded from a channel into a linked group, `msg.from` is "Channel_Bot".
/// In that case we check (in order):
/// 1. `author_signature` — set by Telegram for signed channel posts
/// 2. `sender_chat.title` / `sender_chat.username` — the channel identity
/// 3. `forward_from` — the original user who sent the message
fn extract_sender(msg: &Message) -> (String, String) {
    let from_user = msg.from.as_ref();
    let from_username = from_user
        .and_then(|u| u.username.as_deref())
        .unwrap_or("unknown");
    let from_id = from_user.map(|u| u.id.0.to_string()).unwrap_or_default();

    // If sender is not Channel_Bot, use it directly.
    if from_username != "Channel_Bot" && from_username != "GroupAnonymousBot" {
        return (from_username.to_string(), from_id);
    }

    // Try author_signature (e.g. "Alice" for signed channel posts).
    if let MessageKind::Common(common) = &msg.kind {
        if let Some(sig) = &common.author_signature {
            if !sig.is_empty() {
                return (sig.clone(), from_id);
            }
        }
    }

    // Try sender_chat (the channel/group that "sent" this message).
    if let Some(sender_chat) = &msg.sender_chat {
        if let Some(username) = &sender_chat.username() {
            return (username.to_string(), sender_chat.id.0.to_string());
        }
        if let Some(title) = sender_chat.title() {
            return (title.to_string(), sender_chat.id.0.to_string());
        }
    }

    // Try forward_from_user.
    if let Some(user) = msg.forward_from_user() {
        let name = user
            .username
            .as_deref()
            .unwrap_or(&user.first_name);
        return (name.to_string(), user.id.0.to_string());
    }

    // Fallback.
    (from_username.to_string(), from_id)
}

/// Choose which sticker asset to download for model context. Returns
/// `(file_id, unique_id, mime_type, extension)` or `None` if the sticker
/// has no suitable static asset (e.g. animated without thumbnail).
fn sticker_download_target(
    sticker: &teloxide::types::Sticker,
) -> Option<(&str, &str, &'static str, &'static str)> {
    if let Some(thumb) = &sticker.thumbnail {
        return Some((&thumb.file.id, &thumb.file.unique_id, "image/jpeg", "jpg"));
    }
    if !sticker.is_animated() && !sticker.is_video() {
        return Some((
            &sticker.file.id,
            &sticker.file.unique_id,
            "image/webp",
            "webp",
        ));
    }
    None
}

/// Download a file from Telegram by file_id.
async fn download_file(bot: &Bot, file_id: &str) -> Result<Vec<u8>, teloxide::RequestError> {
    let file = bot.get_file(file_id).await?;
    let mut buf = Vec::new();
    bot.download_file(&file.path, &mut buf).await?;
    Ok(buf)
}

/// Check whether the bot was mentioned in this message.
async fn detect_mention(bot: &Bot, msg: &Message, text: &str) -> bool {
    if msg.chat.is_private() {
        return true;
    }
    if msg
        .reply_to_message()
        .and_then(|r| r.from.as_ref())
        .is_some_and(|u| u.is_bot)
    {
        return true;
    }
    let bot_username = bot
        .get_me()
        .await
        .map(|me| me.username.clone().unwrap_or_default())
        .unwrap_or_default();
    !bot_username.is_empty() && text.contains(&format!("@{bot_username}"))
}

/// Dispatch a buffered media group as a single IncomingMessage with all images merged.
async fn dispatch_media_group(
    messages: Vec<(String, Vec<ImageAttachment>, Message)>,
    config: &NexalConfig,
    on_message: &nexal_channel_core::MessageCallback,
    bot: &Bot,
) {
    if messages.is_empty() {
        return;
    }

    // Use the first message for metadata (chat_id, sender, timestamp, message_id).
    let first_msg = &messages[0].2;
    let chat_id = first_msg.chat.id.0.to_string();
    let username = first_msg
        .from
        .as_ref()
        .and_then(|u| u.username.as_deref())
        .unwrap_or("unknown");

    // Combine all captions (skip empty).
    let combined_text: String = messages
        .iter()
        .map(|(text, _, _)| text.as_str())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // Merge all images.
    let all_images: Vec<ImageAttachment> = messages
        .iter()
        .flat_map(|(_, imgs, _)| imgs.clone())
        .collect();

    let display_text = if combined_text.is_empty() {
        format!("[album: {} image(s)]", all_images.len())
    } else {
        combined_text.clone()
    };

    info!(
        "telegram album from @{username} in {chat_id}: {} ({} image(s))",
        if display_text.len() > 50 { &display_text[..50] } else { &display_text },
        all_images.len()
    );

    let text = if combined_text.is_empty() {
        format!("[received album with {} image(s)]", all_images.len())
    } else {
        combined_text
    };

    let is_mentioned = detect_mention(bot, first_msg, &text).await;
    let is_admin = config.is_admin(&username);

    let incoming = IncomingMessage {
        channel: "telegram".to_string(),
        chat_id,
        sender: username.to_string(),
        text,
        timestamp: first_msg.date.timestamp_millis(),
        is_mentioned,
        metadata: serde_json::json!({
            "message_id": first_msg.id.0,
            "is_admin": is_admin,
        }),
        images: all_images,
    };

    on_message(incoming);
}
