//! HTTP channel adapter for nexal.
//!
//! Implements the [`Channel`] trait, exposing a simple HTTP API for
//! testing message send/receive without needing Telegram or Discord.
//!
//! In headless mode, the agent sends responses via skill scripts that
//! connect to a Unix socket at `/workspace/agents/proxy/http.channel`.
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
    /// Shared outbox: responses from the agent, polled via GET /messages.
    outbox: Outbox,
}

type Outbox = Arc<Mutex<HashMap<String, Vec<String>>>>;

impl HttpChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        Self {
            config,
            outbox: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

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

/// JSON body accepted on the response socket from skill scripts.
#[derive(Deserialize)]
struct SocketResponse {
    chat_id: String,
    text: String,
}

#[async_trait::async_trait]
impl Channel for HttpChannel {
    fn name(&self) -> &str {
        "http"
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let port = self.config.http_channel_port.unwrap_or(3000);
        let outbox = Arc::clone(&self.outbox);

        let state = AppState {
            on_message: Arc::new(on_message),
            outbox: Arc::clone(&outbox),
        };

        let app = Router::new()
            .route("/send", post(handle_send))
            .route("/messages", get(handle_messages))
            .with_state(state);

        // Start the response socket for skill scripts.
        let socket_outbox = Arc::clone(&outbox);
        let workspace = self.config.workspace.clone();
        tokio::spawn(async move {
            if let Err(e) = run_response_socket(&workspace, socket_outbox).await {
                tracing::warn!("http response socket error: {e}");
            }
        });

        let addr = format!("0.0.0.0:{port}");
        info!("HTTP channel listening on {addr}");

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    async fn send(&self, chat_id: &str, text: &str) -> anyhow::Result<()> {
        let mut outbox = self.outbox.lock().await;
        outbox
            .entry(chat_id.to_string())
            .or_default()
            .push(text.to_string());
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

/// Unix socket at `<workspace>/agents/proxy/http.channel` that skill scripts
/// connect to in order to send responses.
///
/// Accepts minimal HTTP POST with JSON body `{"chat_id":"...","text":"..."}`.
/// Pushes the response into the shared outbox.
async fn run_response_socket(
    workspace: &std::path::Path,
    outbox: Outbox,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let socket_path = workspace.join("agents").join("proxy").join("http.channel");
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = tokio::net::UnixListener::bind(&socket_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o777));
    }

    info!(path = %socket_path.display(), "http response socket started");

    loop {
        let (stream, _) = listener.accept().await?;
        let outbox = Arc::clone(&outbox);

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            // Read request line: POST /response HTTP/1.1
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).await.is_err() {
                return;
            }

            // Read headers to find Content-Length
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_err() {
                    return;
                }
                if line.trim().is_empty() {
                    break;
                }
                if let Some(val) = line
                    .strip_prefix("Content-Length:")
                    .or_else(|| line.strip_prefix("content-length:"))
                {
                    content_length = val.trim().parse().unwrap_or(0);
                }
            }

            // Read body
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                use tokio::io::AsyncReadExt;
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }
            }

            // Parse and push to outbox
            let resp = match serde_json::from_slice::<SocketResponse>(&body) {
                Ok(r) => r,
                Err(e) => {
                    let err_resp = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
                        e.to_string().len(),
                        e
                    );
                    let _ = writer.write_all(err_resp.as_bytes()).await;
                    return;
                }
            };

            {
                let mut outbox = outbox.lock().await;
                outbox
                    .entry(resp.chat_id.clone())
                    .or_default()
                    .push(resp.text);
            }

            let ok_body = r#"{"ok":true}"#;
            let ok_resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                ok_body.len(),
                ok_body
            );
            let _ = writer.write_all(ok_resp.as_bytes()).await;
        });
    }
}
