//! Session-scoped message debouncing and batching.
//!
//! Ports the Python `SessionRunner` (from `nexal/channels/runner.py`)
//! into async Rust with three distinct timing states:
//!
//! 1. **Mentioned**: bot was @-mentioned → wait `debounce_secs`, then process.
//! 2. **Active window**: within `active_window_secs` of last mention and
//!    unmentioned follow-up arrives → accumulate, wait `delay_secs`.
//! 3. **Outside window**: unmentioned message arrives after the active window
//!    has elapsed → silently drop.
//!
//! When the timer fires, all pending messages are merged into one
//! [`IncomingMessage`] (text joined with `\n`, last message's metadata wins)
//! and dispatched to the handler.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::IncomingMessage;

/// Timing parameters for the debounce logic.
#[derive(Debug, Clone)]
pub struct DebounceConfig {
    /// How long to wait after a mention before processing (seconds).
    pub debounce_secs: f64,
    /// How long to wait for follow-up messages after a mention (seconds).
    pub delay_secs: f64,
    /// Duration of the "active conversation" window after a mention (seconds).
    pub active_window_secs: f64,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            debounce_secs: 1.0,
            delay_secs: 10.0,
            active_window_secs: 60.0,
        }
    }
}

/// Handler callback: receives a merged [`IncomingMessage`] and processes it.
pub type MessageHandler =
    Arc<dyn Fn(IncomingMessage) -> tokio::task::JoinHandle<()> + Send + Sync>;

/// Per-session debouncer that aggregates messages before dispatching.
pub struct SessionRunner {
    session_id: String,
    config: DebounceConfig,
    inner: Arc<Mutex<Inner>>,
    handler: MessageHandler,
}

struct Inner {
    pending: Vec<IncomingMessage>,
    last_mentioned_at: Option<Instant>,
    /// Handle to the active timer task so we can cancel it.
    timer_handle: Option<tokio::task::JoinHandle<()>>,
}

impl SessionRunner {
    pub fn new(
        session_id: impl Into<String>,
        config: DebounceConfig,
        handler: MessageHandler,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            config,
            inner: Arc::new(Mutex::new(Inner {
                pending: Vec::new(),
                last_mentioned_at: None,
                timer_handle: None,
            })),
            handler,
        }
    }

    /// Process an incoming message through the debounce state machine.
    pub async fn process_message(&self, msg: IncomingMessage) {
        let mut inner = self.inner.lock().await;

        if msg.is_mentioned {
            // State 1: Mentioned → record time, add to pending, set short timer.
            inner.last_mentioned_at = Some(Instant::now());
            inner.pending.push(msg);
            self.reset_timer(&mut inner, self.config.debounce_secs);
            debug!(
                session = %self.session_id,
                "mentioned, debounce timer set for {}s",
                self.config.debounce_secs
            );
        } else if let Some(last) = inner.last_mentioned_at {
            let elapsed = last.elapsed();
            if elapsed < Duration::from_secs_f64(self.config.active_window_secs) {
                // State 2: Within active window → accumulate, reset to longer timer.
                inner.pending.push(msg);
                self.reset_timer(&mut inner, self.config.delay_secs);
                debug!(
                    session = %self.session_id,
                    "active window follow-up, delay timer set for {}s",
                    self.config.delay_secs
                );
            } else {
                // State 3: Outside active window → drop.
                debug!(
                    session = %self.session_id,
                    "message outside active window ({:.1}s elapsed), dropping",
                    elapsed.as_secs_f64()
                );
            }
        } else {
            // No prior mention → drop.
            debug!(
                session = %self.session_id,
                "no prior mention, dropping message"
            );
        }
    }

    /// Cancel any existing timer and start a new one.
    fn reset_timer(&self, inner: &mut Inner, delay_secs: f64) {
        // Cancel existing timer.
        if let Some(handle) = inner.timer_handle.take() {
            handle.abort();
        }

        let state = Arc::clone(&self.inner);
        let handler = Arc::clone(&self.handler);
        let session_id = self.session_id.clone();

        inner.timer_handle = Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;

            // Timer fired — drain pending and dispatch.
            let merged = {
                let mut inner = state.lock().await;
                inner.timer_handle = None;
                let pending = std::mem::take(&mut inner.pending);
                merge_messages(pending)
            };

            if let Some(msg) = merged {
                debug!(session = %session_id, "dispatching merged message");
                let join = (handler)(msg);
                if let Err(e) = join.await {
                    warn!(session = %session_id, "handler panicked: {e}");
                }
            }
        }));
    }
}

/// Merge a list of pending messages into a single message.
///
/// Strategy: use the **last** message as the base (most recent context),
/// combine all texts with newlines, set `is_mentioned = true`.
fn merge_messages(mut messages: Vec<IncomingMessage>) -> Option<IncomingMessage> {
    if messages.is_empty() {
        return None;
    }
    if messages.len() == 1 {
        return messages.pop();
    }

    // Collect all texts in order.
    let combined_text: String = messages
        .iter()
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    // Use the last message as base.
    let mut merged = messages.pop().unwrap();
    merged.text = combined_text;
    merged.is_mentioned = true;

    // Merge images from all messages.
    let mut all_images: Vec<_> = messages
        .into_iter()
        .flat_map(|m| m.images)
        .collect();
    all_images.append(&mut merged.images);
    merged.images = all_images;

    Some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_single_message() {
        let msg = IncomingMessage::new("test", "1", "alice", "hello");
        let result = merge_messages(vec![msg]).unwrap();
        assert_eq!(result.text, "hello");
    }

    #[test]
    fn merge_multiple_messages() {
        let m1 = IncomingMessage::new("test", "1", "alice", "first");
        let m2 = IncomingMessage::new("test", "1", "bob", "second");
        let m3 = IncomingMessage::new("test", "1", "alice", "third");

        let result = merge_messages(vec![m1, m2, m3]).unwrap();
        assert_eq!(result.text, "first\nsecond\nthird");
        assert_eq!(result.sender, "alice"); // last message's sender
        assert!(result.is_mentioned);
    }

    #[test]
    fn merge_empty() {
        assert!(merge_messages(vec![]).is_none());
    }
}
