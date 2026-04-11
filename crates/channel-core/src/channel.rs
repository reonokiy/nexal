//! The [`Channel`] trait — abstract interface for message sources.

use tokio_util::sync::CancellationToken;

use crate::IncomingMessage;

/// Handle that keeps a "typing" indicator alive. When dropped, the
/// typing indicator stops.
pub struct TypingHandle {
    cancel: CancellationToken,
}

impl TypingHandle {
    pub fn new(cancel: CancellationToken) -> Self {
        Self { cancel }
    }
}

impl Drop for TypingHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// A message source (Telegram, Discord, CLI, etc.).
///
/// Channels are long-lived: [`start`](Channel::start) runs until the channel
/// shuts down, calling `on_message` for each incoming message.
#[async_trait::async_trait]
pub trait Channel: Send + Sync + 'static {
    /// Unique identifier for this channel, e.g. `"telegram"`.
    fn name(&self) -> &str;

    /// If `true`, the agent's final text response is automatically sent
    /// back via [`send`](Channel::send).  CLI uses this; Telegram/Discord
    /// instead send messages through skill scripts inside the sandbox.
    fn direct_response(&self) -> bool {
        false
    }

    /// Start listening for messages indefinitely.
    ///
    /// The implementation should call `on_message` for each received message.
    /// This method should block (async) until the channel shuts down.
    async fn start(
        &self,
        on_message: MessageCallback,
    ) -> anyhow::Result<()>;

    /// Send a text message to a specific chat.
    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()>;

    /// Signal that the bot is "typing" in a chat. Implementations should
    /// keep sending the typing indicator until the returned handle is dropped.
    /// Default: no-op.
    fn start_typing(&self, _chat_id: &str) -> Option<TypingHandle> {
        None
    }

    /// Graceful shutdown.
    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Callback type for [`Channel::start`]: receives a message, spawns handling.
///
/// Returns a `JoinHandle` so the channel can optionally track tasks, but
/// typically the result is just ignored (fire-and-forget).
pub type MessageCallback =
    Box<dyn Fn(IncomingMessage) -> tokio::task::JoinHandle<()> + Send + Sync>;

