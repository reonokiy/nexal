//! Session-scoped message debouncing and batching.
//!
//! Ports the Python `SessionRunner` (from `nexal/channels/runner.py`)
//! into async Rust with three distinct timing states:
//!
//! 1. **Mentioned**: bot was @-mentioned → wait `debounce_secs`, then process.
//! 2. **Active window**: within `active_window_secs` of last mention and
//!    unmentioned follow-up arrives → accumulate, wait `delay_secs`.
//! 3. **Outside window**: unmentioned message arrives after the active window
//!    has elapsed → forward with a short delay, let the model decide.
//!
//! When the timer fires, all pending messages are merged into one
//! [`IncomingMessage`] (text joined with `\n`, last message's metadata wins)
//! and dispatched to the handler.
//!
//! ## Actor design
//!
//! Each [`SessionRunner`] owns a background task that holds all mutable
//! state (pending queue, last-mention time, deadline). Public API sends
//! messages over an `mpsc` channel; no `Arc<Mutex<..>>`, no cross-task
//! cloning of state handles.

use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::IncomingMessage;

/// Delay before forwarding unmentioned messages (seconds).
const UNMENTIONED_DELAY_SECS: f64 = 0.1;

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
    std::sync::Arc<dyn Fn(IncomingMessage) -> tokio::task::JoinHandle<()> + Send + Sync>;

/// Per-session debouncer that aggregates messages before dispatching.
///
/// Spawns a background task on construction; the task lives until the
/// `SessionRunner` is dropped (closing the channel).
pub struct SessionRunner {
    tx: mpsc::Sender<IncomingMessage>,
}

impl SessionRunner {
    pub fn new(
        session_id: impl Into<String>,
        config: DebounceConfig,
        handler: MessageHandler,
    ) -> Self {
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(run_actor(session_id.into(), config, handler, rx));
        Self { tx }
    }

    /// Hand a message to the debounce state machine.
    pub async fn process_message(&self, msg: IncomingMessage) {
        if self.tx.send(msg).await.is_err() {
            warn!("session runner actor has exited; message dropped");
        }
    }
}

/// The debounce actor: owns all state, drives the timer, dispatches
/// merged messages through the handler. Runs until the channel closes.
async fn run_actor(
    session_id: String,
    config: DebounceConfig,
    handler: MessageHandler,
    mut rx: mpsc::Receiver<IncomingMessage>,
) {
    let mut pending: Vec<IncomingMessage> = Vec::new();
    let mut last_mentioned_at: Option<Instant> = None;
    let mut deadline: Option<Instant> = None;

    loop {
        tokio::select! {
            maybe_msg = rx.recv() => {
                let Some(msg) = maybe_msg else {
                    debug!(session = %session_id, "session runner shutting down");
                    return;
                };
                deadline = Some(
                    Instant::now()
                        + Duration::from_secs_f64(next_delay_secs(
                            &msg,
                            &config,
                            &mut last_mentioned_at,
                            &session_id,
                        )),
                );
                pending.push(msg);
            }
            _ = wait_until(deadline) => {
                let batch = std::mem::take(&mut pending);
                deadline = None;
                let Some(merged) = merge_messages(batch) else {
                    continue;
                };
                debug!(session = %session_id, "dispatching merged message");
                let join = (handler)(merged);
                if let Err(e) = join.await {
                    warn!(session = %session_id, "handler panicked: {e}");
                }
            }
        }
    }
}

/// Decide how long to wait before dispatching after `msg` arrives, and
/// update `last_mentioned_at` if this is a fresh mention.
fn next_delay_secs(
    msg: &IncomingMessage,
    config: &DebounceConfig,
    last_mentioned_at: &mut Option<Instant>,
    session_id: &str,
) -> f64 {
    if msg.is_mentioned {
        *last_mentioned_at = Some(Instant::now());
        debug!(
            session = %session_id,
            "mentioned, debounce timer set for {}s",
            config.debounce_secs
        );
        return config.debounce_secs;
    }

    if let Some(last) = *last_mentioned_at {
        if last.elapsed() < Duration::from_secs_f64(config.active_window_secs) {
            debug!(
                session = %session_id,
                "active window follow-up, delay timer set for {}s",
                config.delay_secs
            );
            return config.delay_secs;
        }
    }

    debug!(
        session = %session_id,
        "unmentioned message, forwarding with {UNMENTIONED_DELAY_SECS}s delay",
    );
    UNMENTIONED_DELAY_SECS
}

/// Resolve when `deadline` is `Some`, pend forever otherwise. Used as one
/// arm of the actor's `select!` so "no pending batch" naturally blocks on
/// the other arm instead.
async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d.into()).await,
        None => std::future::pending::<()>().await,
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
    let mut merged = messages.pop()?;
    merged.text = combined_text;
    merged.is_mentioned = true;

    // Merge images from all messages.
    let mut all_images: Vec<_> = messages.into_iter().flat_map(|m| m.images).collect();
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
