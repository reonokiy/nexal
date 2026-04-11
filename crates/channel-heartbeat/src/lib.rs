//! Heartbeat channel — periodic wake-ups for proactive agent behavior.
//!
//! Fires a system message into session `"heartbeat:main"` at a regular
//! interval (default 30 min), giving the agent a chance to check on
//! pending tasks, send reminders, or surface anything important.

pub mod config;

use std::sync::Arc;
use std::time::Duration;

use config::HeartbeatChannelConfig;
use nexal_channel_core::{Channel, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use tracing::info;

/// Default heartbeat interval: 30 minutes.
const DEFAULT_INTERVAL_MINS: u64 = 30;

pub struct HeartbeatChannel {
    ch_config: HeartbeatChannelConfig,
}

impl HeartbeatChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        Self {
            ch_config: HeartbeatChannelConfig::from_nexal_config(&config),
        }
    }

    fn interval(&self) -> Duration {
        let mins = self.ch_config.interval_mins.unwrap_or(DEFAULT_INTERVAL_MINS);
        Duration::from_secs(mins * 60)
    }
}

#[async_trait::async_trait]
impl Channel for HeartbeatChannel {
    fn name(&self) -> &str {
        "heartbeat"
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let interval = self.interval();
        info!(
            interval_mins = interval.as_secs() / 60,
            "heartbeat channel started"
        );

        let mut ticker = tokio::time::interval(interval);
        // Skip the first immediate tick — let the system settle on startup.
        ticker.tick().await;

        loop {
            ticker.tick().await;

            info!("heartbeat firing");

            let msg = IncomingMessage::new(
                "heartbeat",
                "main",
                "system",
                "[heartbeat] This is a periodic check-in. Review pending tasks, \
                 conversations, and proactively handle anything that needs attention. \
                 If there is nothing to do, call no_response.",
            );

            on_message(msg);
        }
    }
}
