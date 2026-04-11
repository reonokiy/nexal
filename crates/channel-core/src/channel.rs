//! The [`Channel`] trait — the *input* half of the bot's I/O.
//!
//! Channels are **input-only**. Anything the agent sends back to the user
//! runs inside the sandbox container as a skill script that talks to a
//! host-side Unix-socket proxy (`crates/agent/src/proxy.rs`). The Channel
//! trait therefore has no `send()` method — a channel's only job is to
//! observe an external source and emit [`IncomingMessage`]s.
//!
//! The one output-side concession is [`Channel::start_typing`]: some
//! channels (Telegram) need a host-side task to keep a "typing…" indicator
//! alive while the agent thinks, and that indicator comes from the same
//! client library that delivers messages in.

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

/// A message source (Telegram, Discord, HTTP, heartbeat, cron, …).
///
/// Channels are long-lived: [`start`](Channel::start) runs until the channel
/// shuts down, calling `on_message` for each received message.
#[async_trait::async_trait]
pub trait Channel: Send + Sync + 'static {
    /// Unique identifier for this channel, e.g. `"telegram"`.
    fn name(&self) -> &str;

    /// Start listening for messages indefinitely.
    ///
    /// The implementation should call `on_message` for each received message.
    /// This method should block (async) until the channel shuts down.
    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()>;

    /// Signal that the bot is "typing" in a chat. Implementations should
    /// keep sending the typing indicator until the returned handle is dropped.
    /// Default: no-op.
    fn start_typing(&self, _chat_id: &str) -> Option<TypingHandle> {
        None
    }
}

/// Callback type for [`Channel::start`]: receives a message, spawns handling.
///
/// Returns a `JoinHandle` so the channel can optionally track tasks, but
/// typically the result is just ignored (fire-and-forget).
pub type MessageCallback =
    Box<dyn Fn(IncomingMessage) -> tokio::task::JoinHandle<()> + Send + Sync>;

