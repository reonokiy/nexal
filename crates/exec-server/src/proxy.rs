//! HTTP reverse proxy over Unix sockets.
//!
//! Listens on a Unix socket and forwards HTTP requests to an upstream URL,
//! injecting configured headers (e.g. auth tokens).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// A running proxy instance.
struct ProxyInstance {
    cancel: CancellationToken,
    _task: tokio::task::JoinHandle<()>,
}

/// Manages multiple proxy Unix sockets.
pub(crate) struct ProxyManager {
    instances: tokio::sync::Mutex<HashMap<PathBuf, ProxyInstance>>,
    http_client: reqwest::Client,
}

impl ProxyManager {
    pub fn new() -> Self {
        Self {
            instances: tokio::sync::Mutex::new(HashMap::new()),
            // Disable auto-decompression so we forward raw response bytes as-is.
            http_client: reqwest::Client::builder()
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .no_zstd()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub async fn register(
        &self,
        socket_path: &str,
        upstream_url: &str,
        headers: HashMap<String, String>,
    ) -> Result<(), String> {
        let path = PathBuf::from(socket_path);

        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create proxy dir: {e}"))?;
        }

        // Remove stale socket file.
        let _ = tokio::fs::remove_file(&path).await;

        let listener = UnixListener::bind(&path)
            .map_err(|e| format!("bind {socket_path}: {e}"))?;

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let upstream = upstream_url.to_string();
        let client = self.http_client.clone();

        info!(socket = %socket_path, upstream = %upstream_url, "proxy registered");

        let task = tokio::spawn(async move {
            run_proxy_listener(listener, &upstream, headers, client, cancel_clone).await;
        });

        let mut instances = self.instances.lock().await;
        if let Some(old) = instances.remove(&path) {
            old.cancel.cancel();
        }
        instances.insert(path, ProxyInstance { cancel, _task: task });

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

async fn run_proxy_listener(
    listener: UnixListener,
    upstream: &str,
    headers: HashMap<String, String>,
    client: reqwest::Client,
    cancel: CancellationToken,
) {
    let upstream = Arc::new(upstream.to_string());
    let headers = Arc::new(headers);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let upstream = Arc::clone(&upstream);
                        let headers = Arc::clone(&headers);
                        let client = client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_proxy_connection(stream, &upstream, &headers, &client).await {
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

/// Handle a single HTTP request over a Unix socket connection.
///
/// Reads an HTTP request, forwards it to the upstream, and writes the response back.
/// Supports simple HTTP/1.1 request/response (one per connection).
async fn handle_proxy_connection(
    mut stream: tokio::net::UnixStream,
    upstream: &str,
    inject_headers: &HashMap<String, String>,
    client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read the full HTTP request.
    let mut buf = Vec::with_capacity(8192);
    let mut tmp = [0u8; 8192];
    let headers_end;

    // Read until we find \r\n\r\n (end of headers).
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);

        if let Some(pos) = find_header_end(&buf) {
            headers_end = Some(pos);
            break;
        }
        if buf.len() > 1024 * 1024 {
            return Err("request too large".into());
        }
    }

    let headers_end = headers_end.ok_or("no headers found")?;
    let header_bytes = &buf[..headers_end];
    let header_str = std::str::from_utf8(header_bytes)?;

    // Parse request line and headers.
    let mut lines = header_str.split("\r\n");
    let request_line = lines.next().ok_or("empty request")?;
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().ok_or("no method")?;
    let path = parts.next().ok_or("no path")?;

    let mut content_length: usize = 0;
    let mut req_headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            if key.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            // Skip headers that the proxy handles itself.
            if key.eq_ignore_ascii_case("host")
                || key.eq_ignore_ascii_case("accept-encoding")
            {
                continue;
            }
            req_headers.push((key.to_string(), value.to_string()));
        }
    }

    // Read remaining body if content-length > 0.
    let body_start = headers_end + 4; // skip \r\n\r\n
    let mut body = buf[body_start..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }

    // Build upstream URL.
    let url = format!("{}{}", upstream.trim_end_matches('/'), path);

    // Build request.
    let reqwest_method = method.parse::<reqwest::Method>()?;
    let mut req = client.request(reqwest_method, &url);

    // Add original headers.
    for (k, v) in &req_headers {
        req = req.header(k.as_str(), v.as_str());
    }

    // Inject auth headers (overrides any existing).
    for (k, v) in inject_headers {
        req = req.header(k.as_str(), v.as_str());
    }

    if !body.is_empty() {
        req = req.body(body);
    }

    // Send to upstream.
    let resp = req.send().await?;

    // Write HTTP response back to the Unix socket.
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await?;

    let mut response = format!("HTTP/1.1 {} {}\r\n", status.as_u16(), status.canonical_reason().unwrap_or("OK"));
    for (key, value) in &resp_headers {
        // Skip transfer-encoding — we send the full body at once with content-length.
        if key.as_str().eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        if let Ok(v) = value.to_str() {
            response.push_str(&format!("{}: {}\r\n", key, v));
        }
    }
    if !resp_headers.contains_key("content-length") {
        response.push_str(&format!("content-length: {}\r\n", resp_body.len()));
    }
    response.push_str("connection: close\r\n");
    response.push_str("\r\n");

    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&resp_body).await?;
    stream.flush().await?;

    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
}
