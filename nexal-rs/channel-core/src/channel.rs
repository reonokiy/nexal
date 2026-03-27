//! The [`Channel`] trait — abstract interface for message sources.

use crate::IncomingMessage;

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
        on_message: Box<dyn Fn(IncomingMessage) -> futures_like::BoxSendFut + Send + Sync>,
    ) -> anyhow::Result<()>;

    /// Send a text message to a specific chat.
    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()>;

    /// Graceful shutdown.
    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Helper module for the callback type used in [`Channel::start`].
///
/// We avoid pulling in the full `futures` crate by defining a minimal
/// boxed-future type alias.
mod futures_like {
    use std::future::Future;
    use std::pin::Pin;

    /// A boxed, Send future returning `()`.
    pub type BoxSendFut = Pin<Box<dyn Future<Output = ()> + Send>>;
}

// Re-export for use by channel implementations.
#[allow(unused_imports)]
pub use futures_like::BoxSendFut;
