//! Incoming message types shared across all channels.


/// An image attached to an incoming message.
#[derive(Debug, Clone)]
pub struct ImageAttachment {
    /// Raw image bytes.
    pub data: Vec<u8>,
    /// MIME type, e.g. `"image/jpeg"`.
    pub mime_type: String,
    /// Optional filename.
    pub filename: String,
}

/// A message received from any channel (Telegram, Discord, CLI, etc.).
///
/// This is the unified representation that the agent pool consumes.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Channel identifier, e.g. `"telegram"`, `"discord"`, `"cli"`.
    pub channel: String,
    /// Unique conversation/chat ID within the channel.
    pub chat_id: String,
    /// Human-readable sender name.
    pub sender: String,
    /// Message text content.
    pub text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
    /// Whether the bot was explicitly mentioned (e.g. @bot, reply-to-bot).
    pub is_mentioned: bool,
    /// Channel-specific metadata (message_id, media info, etc.).
    pub metadata: serde_json::Value,
    /// Multimodal image attachments.
    pub images: Vec<ImageAttachment>,
}

impl IncomingMessage {
    /// Create a new message with the minimum required fields.
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        sender: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            chat_id: chat_id.into(),
            sender: sender.into(),
            text: text.into(),
            timestamp: now_millis(),
            is_mentioned: true,
            metadata: serde_json::Value::Null,
            images: Vec::new(),
        }
    }

    /// Unique session key for this message: `"{channel}:{chat_id}"`.
    pub fn session_key(&self) -> String {
        format!("{}:{}", self.channel, self.chat_id)
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
