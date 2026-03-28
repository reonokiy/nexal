//! Bot orchestrator — ties channels, debouncing, and the agent pool together.
//!
//! Messages flow: Channel → SessionRunner → AgentPool.send() (non-blocking)
//! Responses flow: AgentPool event stream → Channel.send()

use std::sync::Arc;

use dashmap::DashMap;
use nexal_channel_core::{
    Channel, DebounceConfig, IncomingMessage, MessageHandler, SessionRunner,
};
use nexal_config::NexalConfig;
use nexal_state::StateDb;
use tracing::{error, info, warn};

use crate::actor::{AgentEvent, AgentMessage};
use crate::AgentPool;

/// Orchestrates channels + agent pool, one per nexal instance.
pub struct Bot {
    pool: Arc<AgentPool>,
    #[allow(dead_code)]
    config: Arc<NexalConfig>,
    #[allow(dead_code)]
    db: Arc<StateDb>,
    channels: Vec<Arc<dyn Channel>>,
    runners: Arc<DashMap<String, Arc<SessionRunner>>>,
    debounce_config: DebounceConfig,
}

impl Bot {
    pub fn new(
        pool: Arc<AgentPool>,
        config: Arc<NexalConfig>,
        #[allow(dead_code)]
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

    pub fn add_channel(&mut self, channel: impl Channel) {
        self.channels.push(Arc::new(channel));
    }

    /// Start all channels + event consumer. Blocks until any channel exits.
    pub async fn run(&self) -> anyhow::Result<()> {
        if self.channels.is_empty() {
            anyhow::bail!("No channels configured.");
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

        // Spawn channel listeners
        for channel in &self.channels {
            let ch = Arc::clone(channel);
            let pool = Arc::clone(&self.pool);
            let runners = Arc::clone(&self.runners);
            let debounce_config = self.debounce_config.clone();

            let handle = tokio::spawn(async move {
                let ch_name = ch.name().to_string();
                let on_message = make_send_handler(
                    Arc::clone(&pool),
                    Arc::clone(&runners),
                    debounce_config,
                );

                if let Err(e) = ch.start(on_message).await {
                    error!(channel = %ch_name, "channel exited with error: {e}");
                }
            });
            handles.push(handle);
        }

        // Spawn event consumer — routes responses back to channels
        let channels: Vec<Arc<dyn Channel>> = self.channels.clone();
        let pool = Arc::clone(&self.pool);
        let event_handle = tokio::spawn(async move {
            while let Some(event) = pool.recv_event().await {
                match event {
                    AgentEvent::Response {
                        session_key,
                        chunks,
                        ..
                    } => {
                        // Parse channel name and chat_id from session_key
                        let (ch_name, chat_id) = match session_key.split_once(':') {
                            Some((c, id)) => (c, id),
                            None => continue,
                        };
                        if let Some(channel) = channels.iter().find(|c| c.name() == ch_name) {
                            for chunk in &chunks {
                                if let Err(e) = channel.send(chat_id, chunk).await {
                                    warn!(session = %session_key, "send error: {e}");
                                }
                            }
                        }
                    }
                    AgentEvent::Error {
                        session_key,
                        message,
                    } => {
                        let (ch_name, chat_id) = match session_key.split_once(':') {
                            Some((c, id)) => (c, id),
                            None => continue,
                        };
                        if let Some(channel) = channels.iter().find(|c| c.name() == ch_name) {
                            let _ = channel.send(chat_id, &format!("Error: {message}")).await;
                        }
                    }
                }
            }
        });
        handles.push(event_handle);

        // Wait for any task to finish
        let (result, _idx, remaining) = futures_select_first(handles).await;
        for h in remaining {
            h.abort();
        }
        result.map_err(|e| anyhow::anyhow!("bot task failed: {e}"))
    }
}

/// Create a handler that sends messages to the pool (non-blocking).
fn make_send_handler(
    pool: Arc<AgentPool>,
    runners: Arc<DashMap<String, Arc<SessionRunner>>>,
    debounce_config: DebounceConfig,
) -> Box<dyn Fn(IncomingMessage) -> tokio::task::JoinHandle<()> + Send + Sync> {
    Box::new(move |msg: IncomingMessage| {
        let session_key = msg.session_key();
        let pool = Arc::clone(&pool);
        let runners = Arc::clone(&runners);
        let debounce_config = debounce_config.clone();

        // Get or create session runner for debouncing
        let runner = runners
            .entry(session_key.clone())
            .or_insert_with(|| {
                let pool_for_handler = Arc::clone(&pool);
                let handler: MessageHandler = Arc::new(move |merged_msg: IncomingMessage| {
                    let pool = Arc::clone(&pool_for_handler);
                    tokio::spawn(async move {
                        let key = merged_msg.session_key();
                        if let Err(e) = pool
                            .send(
                                &key,
                                AgentMessage::UserInput {
                                    text: merged_msg.text,
                                    sender: merged_msg.sender,
                                    channel: merged_msg.channel,
                                },
                            )
                            .await
                        {
                            error!(session = %key, "failed to send to agent: {e}");
                        }
                    })
                });
                Arc::new(SessionRunner::new(session_key.clone(), debounce_config, handler))
            })
            .clone();

        tokio::spawn(async move {
            runner.process_message(msg).await;
        })
    })
}

async fn futures_select_first<T>(
    mut handles: Vec<tokio::task::JoinHandle<T>>,
) -> (
    Result<T, tokio::task::JoinError>,
    usize,
    Vec<tokio::task::JoinHandle<T>>,
) {
    loop {
        for (idx, handle) in handles.iter_mut().enumerate() {
            tokio::select! {
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
