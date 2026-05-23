use std::net::SocketAddr;
use std::sync::Arc;

use nexal_utils_json_transport::JsonMessageConnection;
use rmpv::Value;
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::server::ExecServerHandler;
use crate::server::channel::dispatch;

pub const DEFAULT_LISTEN_URL: &str = "ws://127.0.0.1:0";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ExecServerListenUrlParseError {
    UnsupportedListenUrl(String),
    InvalidListenUrl(String),
}

impl std::fmt::Display for ExecServerListenUrlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecServerListenUrlParseError::UnsupportedListenUrl(url) => write!(
                f,
                "unsupported --listen URL `{url}`; expected `ws://IP:PORT`"
            ),
            ExecServerListenUrlParseError::InvalidListenUrl(url) => {
                write!(f, "invalid --listen URL `{url}`; expected `ws://IP:PORT`")
            }
        }
    }
}

impl std::error::Error for ExecServerListenUrlParseError {}

pub(crate) fn parse_listen_url(
    listen_url: &str,
) -> Result<SocketAddr, ExecServerListenUrlParseError> {
    let addr_str = listen_url.strip_prefix("ws://").ok_or_else(|| {
        ExecServerListenUrlParseError::UnsupportedListenUrl(listen_url.to_string())
    })?;
    addr_str
        .parse::<SocketAddr>()
        .map_err(|_| ExecServerListenUrlParseError::InvalidListenUrl(listen_url.to_string()))
}

pub(crate) async fn run_transport(
    listen_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_address = parse_listen_url(listen_url)?;

    let listener = TcpListener::bind(bind_address).await?;
    let local_addr = listener.local_addr()?;
    info!("nexal-agent listening on ws://{local_addr}");
    println!("ws://{local_addr}");

    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("accept failed: {e}");
                continue;
            }
        };

        tokio::spawn(async move {
            info!("new WebSocket connection from {remote_addr}");
            let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    warn!("websocket handshake failed for {remote_addr}: {e}");
                    return;
                }
            };

            let handler = Arc::new(ExecServerHandler::new());
            let conn = JsonMessageConnection::<Value>::from_websocket_binary(
                ws_stream,
                format!("agent-client-{remote_addr}"),
            );
            let tasks = dispatch::start_dispatch(handler, conn);
            for task in tasks {
                let _ = task.await;
            }
            info!("session dispatch ended for {remote_addr}");
        });
    }
}
