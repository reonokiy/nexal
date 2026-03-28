//! Bot orchestrator — ties channels, debouncing, and the agent pool together.
//!
//! Ports the Python `Bot` class (`nexal/bots/bot.py`):
//! 1. Each channel feeds messages into the Bot.
//! 2. Per-session [`SessionRunner`] debounces/batches messages.
//! 3. Merged messages are dispatched to [`AgentPool::run_turn`].
//! 4. Response chunks are sent back via the channel.

use std::sync::Arc;

use dashmap::DashMap;
use nexal_channel_core::{
    Channel, DebounceConfig, IncomingMessage, MessageHandler, SessionRunner,
};
use nexal_config::NexalConfig;
use nexal_state::StateDb;
use tracing::{error, info, warn};

use crate::AgentPool;

/// Orchestrates channels + agent pool, one per nexal instance.
pub struct Bot {
    pool: Arc<AgentPool>,
    #[allow(dead_code)]
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
    channels: Vec<Arc<dyn Channel>>,
    runners: Arc<DashMap<String, Arc<SessionRunner>>>,
    debounce_config: DebounceConfig,
}

impl Bot {
    pub fn new(
        pool: Arc<AgentPool>,
        config: Arc<NexalConfig>,
        db: Arc<StateDb>,
        debounce_config: DebounceConfig,
    ) -> Self {
        Self {
            pool,
            config,
            db,
            channels: Vec::new(),
            runners: Arc::new(DashMap::new()),
            debounce_config,
        }
    }

    /// Register a channel to listen on.
    pub fn add_channel(&mut self, channel: impl Channel) {
        self.channels.push(Arc::new(channel));
    }

    /// Start all channels concurrently. Blocks until any channel exits.
    pub async fn run(&self) -> anyhow::Result<()> {
        if self.channels.is_empty() {
            anyhow::bail!("No channels configured. Add at least one channel.");
        }

        info!(
            "starting bot with {} channel(s): {}",
            self.channels.len(),
            self.channels
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let mut handles = Vec::new();

        for channel in &self.channels {
            let ch = Arc::clone(channel);
            let pool = Arc::clone(&self.pool);
            let db = Arc::clone(&self.db);
            let runners = Arc::clone(&self.runners);
            let debounce_config = self.debounce_config.clone();

            let handle = tokio::spawn(async move {
                let ch_name = ch.name().to_string();
                let ch_for_send = Arc::clone(&ch);

                let on_message = Box::new(move |msg: IncomingMessage| {
                    let session_key = msg.session_key();
                    let runners = Arc::clone(&runners);
                    let pool = Arc::clone(&pool);
                    let db = Arc::clone(&db);
                    let ch = Arc::clone(&ch_for_send);
                    let debounce_config = debounce_config.clone();

                    // Get or create a SessionRunner for this session.
                    let runner = runners
                        .entry(session_key.clone())
                        .or_insert_with(|| {
                            let handler = make_handler(
                                Arc::clone(&pool),
                                Arc::clone(&db),
                                Arc::clone(&ch),
                            );
                            Arc::new(SessionRunner::new(
                                session_key.clone(),
                                debounce_config,
                                handler,
                            ))
                        })
                        .clone();

                    tokio::spawn(async move {
                        runner.process_message(msg).await;
                    })
                });

                if let Err(e) = ch.start(on_message).await {
                    error!(channel = %ch_name, "channel exited with error: {e}");
                }
            });

            handles.push(handle);
        }

        // Wait for any channel to finish (first one wins).
        let (result, _idx, remaining) = futures_select_first(handles).await;

        // Cancel the rest.
        for h in remaining {
            h.abort();
        }

        result.map_err(|e| anyhow::anyhow!("channel task failed: {e}"))
    }
}

/// Create the handler closure that dispatches merged messages to the agent pool.
fn make_handler(
    pool: Arc<AgentPool>,
    _db: Arc<StateDb>,
    channel: Arc<dyn Channel>,
) -> MessageHandler {
    Arc::new(move |msg: IncomingMessage| {
        let pool = Arc::clone(&pool);
        let channel = Arc::clone(&channel);

        tokio::spawn(async move {
            let session_key = msg.session_key();
            let chat_id = msg.chat_id.clone();

            match pool.run_turn(&session_key, msg.text.clone()).await {
                Ok((chunks, _thread_id)) => {
                    for chunk in chunks {
                        if let Err(e) = channel.send(&chat_id, &chunk).await {
                            warn!(
                                session = %session_key,
                                "failed to send response chunk: {e}"
                            );
                        }
                    }
                }
                Err(e) => {
                    error!(session = %session_key, "agent turn failed: {e}");
                    let _ = channel
                        .send(&chat_id, &format!("Error: {e}"))
                        .await;
                }
            }
        })
    })
}

/// Wait for the first future in a vec of JoinHandles to complete.
async fn futures_select_first<T>(
    mut handles: Vec<tokio::task::JoinHandle<T>>,
) -> (Result<T, tokio::task::JoinError>, usize, Vec<tokio::task::JoinHandle<T>>) {
    use tokio::select;

    // We use a simple loop polling approach.
    loop {
        for (idx, handle) in handles.iter_mut().enumerate() {
            select! {
                result = handle => {
                    let remaining: Vec<_> = handles
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, h)| if i != idx { Some(h) } else { None })
                        .collect();
                    return (result, idx, remaining);
                }
                else => continue,
            }
        }
        tokio::task::yield_now().await;
    }
}
