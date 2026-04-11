//! Host-side token proxy — Unix sockets for API access without exposing tokens.
//!
//! Each service gets its own socket under `<workspace>/agents/proxy/`:
//!   - `proxy/api.telegram.org` — Telegram Bot API
//!   - `proxy/discord.com` — Discord API
//!
//! Skill scripts inside the container connect to the socket.
//! The proxy injects the real token and forwards to the upstream API.
//! Tokens never enter the container.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, error, info, warn};

/// Start all configured proxy servers. Returns handles to stop them.
pub async fn start_proxies(
    workspace: &Path,
    telegram_token: Option<&str>,
    discord_token: Option<&str>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let proxy_dir = workspace.join("agents").join("proxy");
    let _ = tokio::fs::create_dir_all(&proxy_dir).await;

    let mut handles = Vec::new();

    if let Some(token) = telegram_token {
        let sock = proxy_dir.join("api.telegram.org");
        let handle = start_http_proxy(
            sock,
            "https://api.telegram.org".to_string(),
            format!("bot{token}"),
        )
        .await;
        handles.push(handle);
    }

    if let Some(token) = discord_token {
        let sock = proxy_dir.join("discord.com");
        let handle = start_http_proxy(
            sock,
            "https://discord.com".to_string(),
            format!("Bot {token}"),
        )
        .await;
        handles.push(handle);
    }

    handles
}

/// Start a single HTTP proxy on a Unix socket.
///
/// Accepts simple HTTP/1.1 POST requests, injects auth, forwards upstream.
async fn start_http_proxy(
    sock_path: PathBuf,
    upstream_base: String,
    auth_value: String,
) -> tokio::task::JoinHandle<()> {
    // Remove stale socket
    let _ = tokio::fs::remove_file(&sock_path).await;

    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            error!(path = %sock_path.display(), "failed to bind proxy socket: {e}");
            return tokio::spawn(async {});
        }
    };

    // Make socket world-writable so container (different uid) can connect
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o777));
    }

    info!(path = %sock_path.display(), upstream = %upstream_base, "proxy started");

    let upstream_base = Arc::new(upstream_base);
    let auth_value = Arc::new(auth_value);

    tokio::spawn(async move {
        let client = reqwest::Client::new();

        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("proxy accept error: {e}");
                    continue;
                }
            };

            let client = client.clone();
            let upstream = Arc::clone(&upstream_base);
            let auth = Arc::clone(&auth_value);

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, &client, &upstream, &auth).await {
                    debug!("proxy connection error: {e}");
                }
            });
        }
    })
}

/// Handle a single HTTP request over a Unix socket connection.
///
/// Parses a minimal HTTP/1.1 POST, forwards to upstream with auth, returns response.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    client: &reqwest::Client,
    upstream_base: &str,
    auth_value: &str,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Read request line: POST /path HTTP/1.1
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        writer.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    // Read headers
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        if let Some(val) = line.strip_prefix("Content-Length:").or_else(|| line.strip_prefix("content-length:")) {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).await?;
    }

    // Forward to upstream
    let url = format!("{upstream_base}/{auth_value}/{}", path.trim_start_matches('/'));

    let upstream_resp = match method {
        "POST" => {
            client
                .post(&url)
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
        }
        "GET" => client.get(&url).send().await,
        _ => {
            writer
                .write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")
                .await?;
            return Ok(());
        }
    };

    let resp = match upstream_resp {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("HTTP/1.1 502 Bad Gateway\r\n\r\n{{\"error\":\"{e}\"}}");
            writer.write_all(msg.as_bytes()).await?;
            return Ok(());
        }
    };

    let status = resp.status();
    let resp_body = resp.bytes().await.unwrap_or_default();

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        resp_body.len()
    );
    writer.write_all(response.as_bytes()).await?;
    writer.write_all(&resp_body).await?;

    Ok(())
}
