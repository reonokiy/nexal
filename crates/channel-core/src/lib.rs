//! Core channel abstraction for nexal.
//!
//! Ports the Python channel mechanism (channels, debouncing, message batching)
//! into idiomatic Rust with async/await.

mod channel;
mod debounce;
mod message;

pub use channel::{Channel, MessageCallback, TypingHandle};
pub use debounce::{DebounceConfig, MessageHandler, SessionRunner};
pub use message::{ImageAttachment, IncomingMessage};
