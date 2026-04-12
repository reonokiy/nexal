//! Agent orchestrator — ties channels, debouncing, and the agent pool together.
//!
//! Input:  Channel → SessionRunner (debounce) → AgentPool.send()
//! Output: agent skill scripts run *inside* the sandbox container and POST
//!         to a host-side Unix-socket proxy. Replies never flow back through
//!         this orchestrator; the agent event stream is only used to drive
//!         UI side effects (typing indicators, status, error logging).

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use futures::future::select_all;
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

        // Event consumer — drives UI side effects only (typing indicators,
        // error logging). Agent replies flow out through skill scripts
        // inside the sandbox container, not through this stream.
        let mut event_rx = self.pool.take_event_rx();
        let channels_by_name: Arc<HashMap<String, Arc<dyn Channel>>> = Arc::new(
            self.channels
                .iter()
                .map(|ch| (ch.name().to_string(), Arc::clone(ch)))
                .collect(),
        );
        let typing_handles: Arc<Mutex<HashMap<String, TypingHandle>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let event_handle = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
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
                        match status.as_str() {
                            "working" => {
                                if let Some(handle) = start_typing_for_session(
                                    &channels_by_name,
                                    &session_key,
                                ) {
                                    typing_handles
                                        .lock()
                                        .await
                                        .insert(session_key.clone(), handle);
                                }
                            }
                            "idle" => {
                                typing_handles.lock().await.remove(&session_key);
                            }
                            _ => {}
                        }
                    }
                }
            }
        });
        handles.push(event_handle);

        // Wait for any task to finish
        let (result, _idx, remaining) = select_all(handles).await;
        for h in remaining {
            h.abort();
        }
        result.map_err(|e| anyhow::anyhow!("agent task failed: {e}"))
    }
}

/// Look up the channel that owns `session_key` and ask it for a typing
/// handle. Returns `None` if the session key is malformed, the channel is
/// not registered, or the channel does not support typing indicators.
fn start_typing_for_session(
    channels_by_name: &HashMap<String, Arc<dyn Channel>>,
    session_key: &str,
) -> Option<TypingHandle> {
    let (channel_name, chat_id) = session_key.split_once(':')?;
    channels_by_name.get(channel_name)?.start_typing(chat_id)
}

/// Create a handler that sends messages to the pool (non-blocking).
///
/// The hot path — an already-known session — performs zero `Arc` clones:
/// the outer closure captures `pool` / `runners` / `debounce_config` by
/// move and dispatches through the cached `SessionRunner`. Clones only
/// happen on the slow path when a new session is being created and a new
/// `SessionRunner` has to be built.
fn make_send_handler(
    pool: Arc<AgentPool>,
    runners: Arc<DashMap<String, Arc<SessionRunner>>>,
    debounce_config: DebounceConfig,
) -> Box<dyn Fn(IncomingMessage) -> tokio::task::JoinHandle<()> + Send + Sync> {
    Box::new(move |msg: IncomingMessage| {
        let session_key = msg.session_key();
        let runner = runners
            .entry(session_key.clone())
            .or_insert_with(|| {
                new_session_runner(
                    session_key.clone(),
                    debounce_config.clone(),
                    Arc::clone(&pool),
                )
            })
            .clone();

        tokio::spawn(async move {
            runner.process_message(msg).await;
        })
    })
}

/// Build a `SessionRunner` whose inner handler forwards merged messages
/// into `pool`. The handler owns its own `Arc<AgentPool>` so the runner
/// can outlive any specific call-site.
fn new_session_runner(
    session_id: String,
    config: DebounceConfig,
    pool: Arc<AgentPool>,
) -> Arc<SessionRunner> {
    let handler: MessageHandler = Arc::new(move |merged: IncomingMessage| {
        let pool = Arc::clone(&pool);
        tokio::spawn(async move {
            let key = merged.session_key();
            if let Err(e) = pool
                .send(
                    &key,
                    AgentMessage::UserInput {
                        text: merged.text,
                        sender: merged.sender,
                        channel: merged.channel,
                        chat_id: merged.chat_id,
                        metadata: merged.metadata,
                        images: merged.images,
                    },
                )
                .await
            {
                error!(session = %key, "failed to send to agent: {e}");
            }
        })
    });
    Arc::new(SessionRunner::new(session_id, config, handler))
}

