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

pub mod config;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use config::HttpChannelConfig;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use nexal_channel_core::{Channel, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use serde::{Deserialize, Serialize};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tower::Service;
use tracing::{debug, info};

/// HTTP channel that implements the [`Channel`] trait.
pub struct HttpChannel {
    ch_config: HttpChannelConfig,
    workspace: std::path::PathBuf,
    /// Shared outbox: responses from the agent, polled via GET /messages.
    outbox: Outbox,
}

type Outbox = Arc<Mutex<HashMap<String, Vec<String>>>>;

impl HttpChannel {
    pub fn new(config: Arc<NexalConfig>) -> Self {
        Self {
            ch_config: HttpChannelConfig::from_nexal_config(&config),
            workspace: config.workspace.clone(),
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
        let port = self.ch_config.port.unwrap_or(3000);
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
        let workspace = self.workspace.clone();
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
/// Accepts HTTP POST to `/` with JSON body `{"chat_id":"...","text":"..."}`.
/// Pushes the response into the shared outbox.
async fn run_response_socket(
    workspace: &std::path::Path,
    outbox: Outbox,
) -> anyhow::Result<()> {
    let socket_path = workspace.join("agents").join("proxy").join("http.channel");
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(&socket_path).await;

    let listener = UnixListener::bind(&socket_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o777));
    }

    info!(path = %socket_path.display(), "http response socket started");

    let app: Router = Router::new()
        .route("/", post(handle_socket_response))
        .route("/response", post(handle_socket_response))
        .with_state(outbox);

    loop {
        let (stream, _) = listener.accept().await?;
        let tower_service = app.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let hyper_service =
                hyper::service::service_fn(move |req: Request<Incoming>| {
                    tower_service.clone().call(req)
                });
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection(io, hyper_service)
                .await
            {
                debug!("http response socket connection error: {e}");
            }
        });
    }
}

/// Handle a single `POST /` on the response socket: push the text into the
/// per-chat outbox so `GET /messages?chat_id=...` returns it next time.
async fn handle_socket_response(
    State(outbox): State<Outbox>,
    Json(resp): Json<SocketResponse>,
) -> Json<SendResponse> {
    let mut outbox = outbox.lock().await;
    outbox
        .entry(resp.chat_id)
        .or_default()
        .push(resp.text);
    Json(SendResponse { ok: true })
}
