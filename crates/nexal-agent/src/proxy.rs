//! HTTP reverse proxy over Unix sockets.
//!
//! Listens on a Unix socket, establishes a TCP connection to the upstream,
//! and bridges bytes bidirectionally. Auth headers are injected by rewriting
//! the first HTTP request before forwarding.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// A running proxy instance.
struct ProxyInstance {
    cancel: CancellationToken,
    #[allow(dead_code)]
    _task: tokio::task::JoinHandle<()>,
}

/// Manages multiple proxy Unix sockets.
pub(crate) struct ProxyManager {
    instances: tokio::sync::Mutex<HashMap<PathBuf, ProxyInstance>>,
}

impl ProxyManager {
    pub fn new() -> Self {
        Self {
            instances: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn register(
        &self,
        socket_path: &str,
        upstream_url: &str,
        headers: HashMap<String, String>,
    ) -> Result<(), String> {
        let path = PathBuf::from(socket_path);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create proxy dir: {e}"))?;
        }

        let _ = tokio::fs::remove_file(&path).await;

        let listener = UnixListener::bind(&path).map_err(|e| format!("bind {socket_path}: {e}"))?;

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        // Parse upstream URL into host:port for TCP connection.
        let upstream_info =
            parse_upstream(upstream_url).map_err(|e| format!("invalid upstream URL: {e}"))?;

        info!(socket = %socket_path, upstream = %upstream_url, "proxy registered");

        let task = tokio::spawn(async move {
            run_proxy_listener(listener, upstream_info, headers, cancel_clone).await;
        });

        let mut instances = self.instances.lock().await;
        if let Some(old) = instances.remove(&path) {
            old.cancel.cancel();
        }
        instances.insert(
            path,
            ProxyInstance {
                cancel,
                _task: task,
            },
        );

        Ok(())
    }

    pub async fn unregister(&self, socket_path: &str) -> bool {
        let path = PathBuf::from(socket_path);
        let mut instances = self.instances.lock().await;
        if let Some(inst) = instances.remove(&path) {
            inst.cancel.cancel();
            let _ = tokio::fs::remove_file(&path).await;
            info!(socket = %socket_path, "proxy unregistered");
            true
        } else {
            false
        }
    }

    pub async fn shutdown(&self) {
        let mut instances = self.instances.lock().await;
        for (path, inst) in instances.drain() {
            inst.cancel.cancel();
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
}

/// Parsed upstream connection info.
#[derive(Clone)]
struct UpstreamInfo {
    /// Host for TCP connection (e.g. "s.jina.ai")
    host: String,
    /// Port (443 for https, 80 for http)
    port: u16,
    /// Whether to use TLS
    tls: bool,
    /// Path prefix from the upstream URL (e.g. "/bot<token>" for Telegram)
    path_prefix: String,
}

fn parse_upstream(url: &str) -> Result<UpstreamInfo, String> {
    let (scheme, rest) = url.split_once("://").ok_or("missing scheme")?;
    let tls = scheme == "https";
    let port = if tls { 443 } else { 80 };

    let (host_port, path_prefix) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, String::new()),
    };

    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().map_err(|_| "invalid port")?),
        None => (host_port.to_string(), port),
    };

    Ok(UpstreamInfo {
        host,
        port,
        tls,
        path_prefix,
    })
}

async fn run_proxy_listener(
    listener: UnixListener,
    upstream: UpstreamInfo,
    headers: HashMap<String, String>,
    cancel: CancellationToken,
) {
    let upstream = Arc::new(upstream);
    let headers = Arc::new(headers);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let upstream = Arc::clone(&upstream);
                        let headers = Arc::clone(&headers);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, &upstream, &headers).await {
                                debug!("proxy connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        warn!("proxy accept error: {e}");
                        break;
                    }
                }
            }
            _ = cancel.cancelled() => break,
        }
    }
}

/// Handle a single proxied connection.
///
/// Reads the first HTTP request from the Unix socket, rewrites it to inject
/// auth headers and the upstream path prefix, then opens a TLS/TCP connection
/// to the upstream and bridges bytes in both directions.
async fn handle_connection(
    mut client: tokio::net::UnixStream,
    upstream: &UpstreamInfo,
    inject_headers: &HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read the HTTP request head from the client.
    let mut buf = Vec::with_capacity(8192);
    let mut tmp = [0u8; 8192];

    let headers_end;
    loop {
        let n = client.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);

        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            headers_end = pos;
            break;
        }
        if buf.len() > 1024 * 1024 {
            return Err("request head too large".into());
        }
    }

    // Parse just the request line to rewrite the path.
    let head = std::str::from_utf8(&buf[..headers_end])?;
    let first_line_end = head.find("\r\n").unwrap_or(head.len());
    let request_line = &head[..first_line_end];
    let rest_headers = &head[first_line_end..]; // includes leading \r\n

    // Rewrite request line: inject path prefix.
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().ok_or("no method")?;
    let path = parts.next().ok_or("no path")?;
    let version = parts.next().unwrap_or("HTTP/1.1");

    let new_path = if upstream.path_prefix.is_empty() {
        path.to_string()
    } else {
        format!("{}{}", upstream.path_prefix.trim_end_matches('/'), path)
    };

    // Build the rewritten request head.
    let mut rewritten = format!("{method} {new_path} {version}\r\n");
    rewritten.push_str(&format!("Host: {}\r\n", upstream.host));

    // Copy original headers (skip Host — we set it above).
    for line in rest_headers.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if let Some((key, _)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case("host") {
                continue;
            }
        }
        rewritten.push_str(line);
        rewritten.push_str("\r\n");
    }

    // Inject auth headers.
    for (key, value) in inject_headers {
        rewritten.push_str(&format!("{key}: {value}\r\n"));
    }

    rewritten.push_str("\r\n");

    // Remaining bytes after the header (start of body, if any).
    let body_start = headers_end + 4;
    let remaining_body = if body_start < buf.len() {
        &buf[body_start..]
    } else {
        &[]
    };

    // Connect to upstream.
    let upstream_addr = format!("{}:{}", upstream.host, upstream.port);
    debug!(upstream = %upstream_addr, tls = upstream.tls, path = %new_path, "proxy connecting to upstream");

    if upstream.tls {
        // TLS connection.
        let tcp = TcpStream::connect(&upstream_addr).await?;
        debug!("TCP connected, starting TLS handshake");
        let connector = tokio_rustls_connector(&upstream.host)?;
        let server_name = rustls::pki_types::ServerName::try_from(upstream.host.clone())?;
        let mut tls_stream = connector.connect(server_name, tcp).await?;
        debug!("TLS handshake complete");

        // Send rewritten head + remaining body.
        tls_stream.write_all(rewritten.as_bytes()).await?;
        if !remaining_body.is_empty() {
            tls_stream.write_all(remaining_body).await?;
        }
        tls_stream.flush().await?;

        // Bridge bidirectionally.
        let (mut tls_read, mut tls_write) = tokio::io::split(tls_stream);
        let (mut client_read, mut client_write) = tokio::io::split(client);

        tokio::select! {
            r = tokio::io::copy(&mut tls_read, &mut client_write) => { r?; }
            r = tokio::io::copy(&mut client_read, &mut tls_write) => { r?; }
        }
    } else {
        // Plain TCP.
        let mut tcp = TcpStream::connect(&upstream_addr).await?;

        tcp.write_all(rewritten.as_bytes()).await?;
        if !remaining_body.is_empty() {
            tcp.write_all(remaining_body).await?;
        }
        tcp.flush().await?;

        let (mut tcp_read, mut tcp_write) = tokio::io::split(tcp);
        let (mut client_read, mut client_write) = tokio::io::split(client);

        tokio::select! {
            r = tokio::io::copy(&mut tcp_read, &mut client_write) => { r?; }
            r = tokio::io::copy(&mut client_read, &mut tcp_write) => { r?; }
        }
    }

    Ok(())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Build a TLS connector using native root certificates.
fn tokio_rustls_connector(
    _host: &str,
) -> Result<tokio_rustls::TlsConnector, Box<dyn std::error::Error>> {
    let mut root_store = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs();
    for cert in certs.certs {
        let _ = root_store.add(cert);
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(root_store)
    .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}
