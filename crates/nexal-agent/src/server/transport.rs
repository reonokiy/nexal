use std::net::SocketAddr;

use jsonrpsee::server::{ServerBuilder, ServerHandle};

use crate::server::ExecServerHandler;
use crate::server::rpc::jsonrpsee::build_module;

pub const DEFAULT_LISTEN_URL: &str = "ws://127.0.0.1:0";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ExecServerListenUrlParseError {
    UnsupportedListenUrl(String),
    InvalidWebSocketListenUrl(String),
}

impl std::fmt::Display for ExecServerListenUrlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecServerListenUrlParseError::UnsupportedListenUrl(listen_url) => write!(
                f,
                "unsupported --listen URL `{listen_url}`; expected `ws://IP:PORT`"
            ),
            ExecServerListenUrlParseError::InvalidWebSocketListenUrl(listen_url) => write!(
                f,
                "invalid websocket --listen URL `{listen_url}`; expected `ws://IP:PORT`"
            ),
        }
    }
}

impl std::error::Error for ExecServerListenUrlParseError {}

pub(crate) fn parse_listen_url(
    listen_url: &str,
) -> Result<SocketAddr, ExecServerListenUrlParseError> {
    if let Some(socket_addr) = listen_url.strip_prefix("ws://") {
        return socket_addr.parse::<SocketAddr>().map_err(|_| {
            ExecServerListenUrlParseError::InvalidWebSocketListenUrl(listen_url.to_string())
        });
    }

    Err(ExecServerListenUrlParseError::UnsupportedListenUrl(
        listen_url.to_string(),
    ))
}

pub(crate) async fn run_transport(
    listen_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_address = parse_listen_url(listen_url)?;
    let (local_addr, handle) = start_server(bind_address).await?;
    tracing::info!("nexal-exec-server listening on ws://{local_addr}");
    println!("ws://{local_addr}");
    handle.stopped().await;
    Ok(())
}

pub(crate) async fn start_server(
    bind_address: SocketAddr,
) -> Result<(SocketAddr, ServerHandle), Box<dyn std::error::Error + Send + Sync>> {
    let server = ServerBuilder::default().build(bind_address).await?;
    let local_addr = server.local_addr()?;
    let handler = std::sync::Arc::new(ExecServerHandler::new());
    let module = build_module(handler.clone());
    let handle = server.start(module);
    let cleanup_handle = handle.clone();
    tokio::spawn(async move {
        cleanup_handle.stopped().await;
        handler.shutdown().await;
    });
    Ok((local_addr, handle))
}
