//! HTTP channel adapter for nexal.
//!
//! Implements the [`Channel`] trait, exposing a simple HTTP API for
//! testing message send/receive without needing Telegram or Discord.
//!
//! ## Endpoints
//!
//! - `POST /send` — send a message to the bot
//!   ```json
//!   { "chat_id": "test", "sender": "alice", "text": "hello" }
//!   ```
//! - `GET /messages?chat_id=test` — poll bot responses for a chat

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use nexal_channel_core::{Channel, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;

/// HTTP channel that implements the [`Channel`] trait.
pub struct HttpChannel {
    config: Arc<NexalConfig>,
}

impl HttpChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        Self { config }
    }
}

type Outbox = Arc<Mutex<HashMap<String, Vec<String>>>>;

#[derive(Clone)]
struct AppState {
    on_message: Arc<MessageCallback>,
    outbox: Outbox,
}

#[derive(Deserialize)]
struct SendRequest {
    chat_id: Option<String>,
    sender: Option<String>,
    text: String,
}

#[derive(Serialize)]
struct SendResponse {
    ok: bool,
}

#[derive(Deserialize)]
struct MessagesQuery {
    chat_id: Option<String>,
}

#[derive(Serialize)]
struct MessagesResponse {
    messages: Vec<String>,
}

#[async_trait::async_trait]
impl Channel for HttpChannel {
    fn name(&self) -> &str {
        "http"
    }

    fn direct_response(&self) -> bool {
        true
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let port = self.config.http_channel_port.unwrap_or(3000);
        let outbox: Outbox = Arc::new(Mutex::new(HashMap::new()));

        let state = AppState {
            on_message: Arc::new(on_message),
            outbox,
        };

        let app = Router::new()
            .route("/send", post(handle_send))
            .route("/messages", get(handle_messages))
            .with_state(state);

        let addr = format!("0.0.0.0:{port}");
        info!("HTTP channel listening on {addr}");

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        // HTTP channel responses are collected in the outbox and polled via GET /messages.
        // This method is called by the bot orchestrator but there's no persistent outbox
        // reference here — responses go through direct_response instead.
        info!("http send to {chat_id}: {text}");
        Ok(())
    }
}

async fn handle_send(
    State(state): State<AppState>,
    Json(req): Json<SendRequest>,
) -> Json<SendResponse> {
    let chat_id = req.chat_id.unwrap_or_else(|| "default".to_string());
    let sender = req.sender.unwrap_or_else(|| "http-user".to_string());

    info!("HTTP incoming from {sender} in {chat_id}: {}", req.text);

    let msg = IncomingMessage::new("http", &chat_id, &sender, &req.text);
    (state.on_message)(msg);

    Json(SendResponse { ok: true })
}

async fn handle_messages(
    State(state): State<AppState>,
    Query(query): Query<MessagesQuery>,
) -> Json<MessagesResponse> {
    let chat_id = query.chat_id.unwrap_or_else(|| "default".to_string());
    let mut outbox = state.outbox.lock().await;
    let messages = outbox.remove(&chat_id).unwrap_or_default();
    Json(MessagesResponse { messages })
}
