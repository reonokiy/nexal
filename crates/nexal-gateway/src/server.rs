//! Frontend server — accepts WebSocket or Unix socket connections.
//!
//! WebSocket (TCP): used for external frontend connections.
//! Also responds to plain HTTP requests (Fly health checks) with 200 OK.
//! Unix socket: uses newline-delimited JSON over a raw stream (no WS).
//!
//! Session lifecycle:
//!   1. First client message MUST be a HMAC-signed `gateway/hello`
//!      `{ access_key, client_name, ts, nonce, signature }`.
//!   2. Once authenticated, the session can call gateway methods,
//!      `agent/invoke`, and receives `agent/notify` notifications.
//!   3. On disconnect, agent containers are detached (kept alive).

mod session;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use nexal_utils_json_transport::JsonMessageConnection;
use rmpv::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::{TcpListener, UnixListener};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::protocol::JsonRpcError;
use crate::protocol::error_code;
use crate::registry::AgentRegistry;

pub const GATEWAY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Wraps an async stream, prepending buffered data before the real stream.
/// Used to replay HTTP request headers already read during inspection.
struct PrefixedStream<S> {
    prefix: io::Cursor<Vec<u8>>,
    inner: S,
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefixedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let pos = self.prefix.position() as usize;
        let data = self.prefix.get_ref();
        if pos < data.len() {
            let remaining = &data[pos..];
            let n = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..n]);
            self.prefix.set_position((pos + n) as u64);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefixedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Clone)]
pub struct ServerConfig {
    pub listen: String,
    pub unix: Option<PathBuf>,
    /// access_key → secret_key. Handshake is HMAC-signed.
    pub credentials: HashMap<String, String>,
    /// Shared replay guard: nonce → first-seen unix seconds.
    pub nonce_cache: Arc<Mutex<HashMap<String, i64>>>,
    pub proxy_external_base: String,
}

pub async fn serve(cfg: ServerConfig, registry: Arc<AgentRegistry>) -> std::io::Result<()> {
    if let Some(ref path) = cfg.unix {
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(path)?;
        info!("nexal-gateway listening on unix:{}", path.display());
        loop {
            let (stream, _addr) = match listener.accept().await {
                Ok(v) => v,
                Err(err) => {
                    error!("accept failed: {err}");
                    continue;
                }
            };
            let label = format!(
                "unix-{}",
                _addr
                    .as_pathname()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            );
            let cfg = cfg.clone();
            let registry = registry.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_unix_stream(stream, &label, cfg, registry).await {
                    warn!("session for {label} ended: {err}");
                }
            });
        }
    } else {
        let listener = TcpListener::bind(&cfg.listen).await?;
        let local_addr = listener.local_addr()?;
        info!("nexal-gateway listening on ws://{local_addr}");

        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok(v) => v,
                Err(err) => {
                    error!("accept failed: {err}");
                    continue;
                }
            };
            let cfg = cfg.clone();
            let registry = registry.clone();
            tokio::spawn(async move {
                let mut buf = BufReader::new(stream);
                let mut headers = Vec::new();
                let mut line = String::new();
                let mut is_ws = false;
                loop {
                    line.clear();
                    match buf.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        _ => {}
                    }
                    headers.extend_from_slice(line.as_bytes());
                    if line.eq_ignore_ascii_case("upgrade: websocket\r\n") {
                        is_ws = true;
                    }
                    if line == "\r\n" {
                        break;
                    }
                }

                if is_ws {
                    let prefixed = PrefixedStream {
                        prefix: io::Cursor::new(headers),
                        inner: buf,
                    };
                    let ws_stream = match tokio_tungstenite::accept_async(prefixed).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            warn!("websocket handshake failed for {remote_addr}: {e}");
                            return;
                        }
                    };
                    let label = format!("ws-{remote_addr}");
                    let conn = JsonMessageConnection::<Value>::from_websocket_binary(
                        ws_stream,
                        format!("frontend ws {label}"),
                    );
                    info!("frontend session opened: {label}");
                    let session = session::Session::from_conn(conn, cfg, registry, label.clone());
                    session.run().await;
                    info!("frontend session closed: {label}");
                } else {
                    let mut inner = buf.into_inner();
                    let _ = inner
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                        .await;
                    debug!("health-check response sent to {remote_addr}");
                }
            });
        }
    }
}

async fn handle_unix_stream<S>(
    stream: S,
    label: &str,
    cfg: ServerConfig,
    registry: Arc<AgentRegistry>,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let conn = JsonMessageConnection::<Value>::from_stdio_binary(
        reader,
        writer,
        format!("frontend unix {label}"),
    );
    info!("frontend session opened: {label}");
    let session = session::Session::from_conn(conn, cfg, registry, label.to_string());
    session.run().await;
    info!("frontend session closed: {label}");
    Ok(())
}

pub(crate) fn parse_params<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, JsonRpcError> {
    rmpv::ext::from_value(value).map_err(|err| JsonRpcError {
        code: error_code::INVALID_PARAMS,
        message: format!("invalid params: {err}"),
        data: None,
    })
}

pub(crate) fn registry_err(err: crate::registry::RegistryError) -> JsonRpcError {
    use crate::registry::RegistryError::*;
    let (code, msg) = match err {
        Backend(e) => (error_code::BACKEND_ERROR, format!("{e}")),
        AgentConn(e) => (error_code::BACKEND_ERROR, format!("{e}")),
        UnknownAgent(id) => (error_code::UNKNOWN_AGENT, format!("unknown agent {id}")),
        UnknownContainer(name) => (
            error_code::UNKNOWN_AGENT,
            format!("unknown container {name}"),
        ),
    };
    JsonRpcError {
        code,
        message: msg,
        data: None,
    }
}

pub(crate) fn container_socket_path(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            sanitized.push(c);
        } else {
            sanitized.push('_');
        }
    }
    format!("/run/nexal/proxy/{sanitized}.socket")
}

#[allow(dead_code)]
fn _ensure_msgpack_used() -> Value {
    Value::Map(vec![])
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::{container_socket_path, parse_params, registry_err};
    use crate::agent_conn::AgentConnError;
    use crate::backend::BackendError;
    use crate::protocol::error_code;
    use crate::registry::RegistryError;

    #[derive(Debug, Deserialize)]
    struct Sample {
        agent_id: String,
        count: u32,
    }

    #[test]
    fn parse_params_succeeds_on_well_formed_json() {
        let s: Sample = parse_params(json!({ "agent_id": "a", "count": 7 }))
            .expect("well-formed params should deserialize");
        assert_eq!(s.agent_id, "a");
        assert_eq!(s.count, 7);
    }

    #[test]
    fn parse_params_wraps_serde_error_as_invalid_params() {
        let err = parse_params::<Sample>(json!({ "agent_id": "a" }))
            .expect_err("should reject missing fields");
        assert_eq!(err.code, error_code::INVALID_PARAMS);
    }

    #[test]
    fn parse_params_rejects_wrong_shape() {
        let err = parse_params::<Sample>(json!([1, 2, 3])).expect_err("array is not an object");
        assert_eq!(err.code, error_code::INVALID_PARAMS);
    }

    #[test]
    fn registry_err_maps_unknown_agent_to_unknown_agent_code() {
        let err = registry_err(RegistryError::UnknownAgent("abc".into()));
        assert_eq!(err.code, error_code::UNKNOWN_AGENT);
    }

    #[test]
    fn registry_err_maps_unknown_container_to_unknown_agent_code() {
        let err = registry_err(RegistryError::UnknownContainer("nexal-x".into()));
        assert_eq!(err.code, error_code::UNKNOWN_AGENT);
    }

    #[test]
    fn registry_err_maps_backend_errors_to_backend_code() {
        let err = registry_err(RegistryError::Backend(BackendError::Cli("fail".into())));
        assert_eq!(err.code, error_code::BACKEND_ERROR);
    }

    #[test]
    fn registry_err_maps_agent_conn_errors_to_backend_code() {
        let err = registry_err(RegistryError::AgentConn(AgentConnError::BadFrame(
            "bad".into(),
        )));
        assert_eq!(err.code, error_code::BACKEND_ERROR);
    }

    #[test]
    fn socket_path_uses_convention() {
        assert_eq!(
            container_socket_path("jina"),
            "/run/nexal/proxy/jina.socket"
        );
    }

    #[test]
    fn socket_path_preserves_safe_chars() {
        assert_eq!(
            container_socket_path("api-v2.0_final"),
            "/run/nexal/proxy/api-v2.0_final.socket"
        );
    }

    #[test]
    fn socket_path_sanitizes_unsafe_chars() {
        assert_eq!(
            container_socket_path("foo/bar baz"),
            "/run/nexal/proxy/foo_bar_baz.socket"
        );
    }

    #[test]
    fn socket_path_preserves_empty_name_safely() {
        assert_eq!(container_socket_path(""), "/run/nexal/proxy/.socket");
    }

    #[test]
    fn socket_path_sanitizes_unicode_to_underscore() {
        assert_eq!(container_socket_path("测试"), "/run/nexal/proxy/__.socket");
    }
}
