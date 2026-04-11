//! Agent orchestrator — ties channels, debouncing, and the agent pool together.
//!
//! Messages flow: Channel → SessionRunner → AgentPool.send() (non-blocking)
//! Responses flow: AgentPool event stream → Channel.send()

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use nexal_channel_core::{
    Channel, DebounceConfig, IncomingMessage, MessageHandler, SessionRunner, TypingHandle,
};
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::actor::{AgentEvent, AgentMessage};
use crate::AgentPool;

/// Orchestrates channels + agent pool, one per nexal instance.
pub struct Agent {
    pool: Arc<AgentPool>,
    channels: Vec<Arc<dyn Channel>>,
    runners: Arc<DashMap<String, Arc<SessionRunner>>>,
    debounce_config: DebounceConfig,
}

impl Agent {
    pub fn new(
        pool: Arc<AgentPool>,
        debounce_config: DebounceConfig,
    ) -> Self {
        Self {
            pool,
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
            "starting agent with {} channel(s): {}",
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

        // Event consumer — agent sends messages via skill scripts (telegram_send.py etc.)
        // through the Unix socket proxy. No automatic channel.send() here.
        let pool = Arc::clone(&self.pool);
        // Build a channel lookup by name so we can call start_typing.
        let channels_by_name: Arc<HashMap<String, Arc<dyn Channel>>> = Arc::new(
            self.channels
                .iter()
                .map(|ch| (ch.name().to_string(), Arc::clone(ch)))
                .collect(),
        );
        let typing_handles: Arc<Mutex<HashMap<String, TypingHandle>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let event_handle = tokio::spawn(async move {
            while let Some(event) = pool.recv_event().await {
                match event {
                    AgentEvent::Response { session_key, chunks, .. } => {
                        tracing::debug!(session = %session_key, "agent turn completed");
                        typing_handles.lock().await.remove(&session_key);

                        // If the model produced a text response (headless mode fallback),
                        // auto-send it to the user via the channel. This happens when the
                        // model gives up on tool calls and writes a text summary.
                        if !chunks.is_empty() {
                            if let Some((channel_name, chat_id)) = session_key.split_once(':') {
                                if let Some(channel) = channels_by_name.get(channel_name) {
                                    for chunk in &chunks {
                                        if let Err(e) = channel.send(chat_id, chunk).await {
                                            tracing::warn!(
                                                session = %session_key,
                                                "failed to send fallback text: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    AgentEvent::Error { session_key, message } => {
                        tracing::error!(session = %session_key, "agent error: {message}");
                        typing_handles.lock().await.remove(&session_key);
                    }
                    AgentEvent::StatusChange { session_key, status, activity } => {
                        tracing::debug!(
                            session = %session_key,
                            status = %status,
                            activity = %activity,
                            "agent status changed"
                        );
                        if status == "working" {
                            // Start typing indicator
                            if let Some((channel_name, chat_id)) = session_key.split_once(':') {
                                if let Some(channel) = channels_by_name.get(channel_name) {
                                    if let Some(handle) = channel.start_typing(chat_id) {
                                        typing_handles.lock().await.insert(session_key.clone(), handle);
                                    }
                                }
                            }
                        } else if status == "idle" {
                            // Stop typing indicator
                            typing_handles.lock().await.remove(&session_key);
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
        result.map_err(|e| anyhow::anyhow!("agent task failed: {e}"))
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
                                    chat_id: merged_msg.chat_id,
                                    metadata: merged_msg.metadata,
                                    images: merged_msg.images,
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
