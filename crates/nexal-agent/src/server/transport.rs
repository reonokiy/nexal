use std::net::SocketAddr;
use std::sync::Arc;

use nexal_utils_json_transport::JsonMessageConnection;
use serde_json::Value;
use tracing::{error, info, warn};
use wtransport::{Endpoint, Identity, ServerConfig};

use crate::server::ExecServerHandler;
use crate::server::rpc::dispatch;

pub const DEFAULT_LISTEN_URL: &str = "wt://127.0.0.1:0";

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
                "unsupported --listen URL `{url}`; expected `wt://IP:PORT` or `ws://IP:PORT`"
            ),
            ExecServerListenUrlParseError::InvalidListenUrl(url) => write!(
                f,
                "invalid --listen URL `{url}`; expected `wt://IP:PORT` or `ws://IP:PORT`"
            ),
        }
    }
}

impl std::error::Error for ExecServerListenUrlParseError {}

pub(crate) fn parse_listen_url(
    listen_url: &str,
) -> Result<SocketAddr, ExecServerListenUrlParseError> {
    let addr_str = listen_url
        .strip_prefix("wt://")
        .or_else(|| listen_url.strip_prefix("ws://"))
        .ok_or_else(|| {
            ExecServerListenUrlParseError::UnsupportedListenUrl(listen_url.to_string())
        })?;
    addr_str.parse::<SocketAddr>().map_err(|_| {
        ExecServerListenUrlParseError::InvalidListenUrl(listen_url.to_string())
    })
}

pub(crate) async fn run_transport(
    listen_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_address = parse_listen_url(listen_url)?;

    // Generate self-signed TLS certs for the agent endpoint.
    let certs = nexal_utils_certs::generate()?;
    let identity = Identity::self_signed([
        "localhost",
        "127.0.0.1",
        "host.containers.internal",
    ])?;

    let config = ServerConfig::builder()
        .with_bind_address(bind_address)
        .with_identity(identity)
        .build();
    // Keep the generated certs around for reference; the actual TLS
    // identity is self-signed via wtransport's built-in helper.
    drop(certs);

    let endpoint = Endpoint::server(config)?;
    let local_addr = endpoint.local_addr()?;
    info!("nexal-agent listening on https://{local_addr}");
    println!("https://{local_addr}");

    loop {
        let incoming = match endpoint.accept().await {
            incoming => incoming,
        };
        let request = match incoming.await {
            Ok(req) => req,
            Err(e) => {
                warn!("webtransport session request failed: {e}");
                continue;
            }
        };
        let session = match request.accept().await {
            Ok(s) => s,
            Err(e) => {
                warn!("webtransport session accept failed: {e}");
                continue;
            }
        };

        tokio::spawn(async move {
            info!("new WebTransport session from {}", session.remote_address());
            match session.accept_bi().await {
                Ok(stream) => {
                    info!("accepted bi-stream");
                    let bi: wtransport::stream::BiStream = stream.into();
                    let handler = Arc::new(ExecServerHandler::new());
                    let conn = JsonMessageConnection::<Value>::from_webtransport(
                        bi,
                        "agent-client".to_string(),
                    );
                    let tasks = dispatch::start_dispatch(handler, conn);
                    for task in tasks {
                        let _ = task.await;
                    }
                    info!("session dispatch ended");
                }
                Err(e) => {
                    error!("accept bidirectional stream failed: {e}");
                }
            }
        });
    }
}

/// Legacy WebSocket server for backward compatibility during migration.
/// Uses jsonrpsee — will be removed after full cutover.
#[cfg(test)]
pub(crate) async fn start_server(
    bind_address: SocketAddr,
) -> Result<(SocketAddr, jsonrpsee::server::ServerHandle), Box<dyn std::error::Error + Send + Sync>>
{
    use jsonrpsee::server::ServerBuilder;
    use crate::server::rpc::jsonrpsee::build_module;

    let server = ServerBuilder::default().build(bind_address).await?;
    let local_addr = server.local_addr()?;
    let handler = Arc::new(ExecServerHandler::new());
    let module = build_module(handler.clone());
    let handle = server.start(module);
    let cleanup_handle = handle.clone();
    tokio::spawn(async move {
        cleanup_handle.stopped().await;
        handler.shutdown().await;
    });
    Ok((local_addr, handle))
}
