#![allow(dead_code)]

use std::process::Stdio;
use std::time::Duration;

use anyhow::anyhow;
use futures::{SinkExt, StreamExt};
use nexal_agent::JSONRPCMessage;
use nexal_agent::JSONRPCNotification;
use nexal_agent::JSONRPCRequest;
use nexal_agent::JsonRpcVersion;
use nexal_agent::RequestId;
use nexal_utils_cargo_bin::cargo_bin;
use serde_json::Value as JsonValue;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::Instant;
use tokio::time::sleep;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ExecServerHarness {
    child: Child,
    url: String,
    ws: WebSocketStream<tokio::net::TcpStream>,
    next_request_id: i64,
}

impl Drop for ExecServerHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

pub(crate) async fn exec_server() -> anyhow::Result<ExecServerHarness> {
    let binary = cargo_bin("nexal-agent")?;
    let mut child = Command::new(binary);
    child.args(["--listen", "ws://127.0.0.1:0"]);
    child.stdin(Stdio::null());
    child.stdout(Stdio::piped());
    child.stderr(Stdio::inherit());
    let mut child = child.spawn()?;

    let url = read_listen_url_from_stdout(&mut child).await?;
    let ws = connect_websocket_when_ready(&url).await?;

    Ok(ExecServerHarness {
        child,
        url,
        ws,
        next_request_id: 1,
    })
}

impl ExecServerHarness {
    pub(crate) async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<RequestId> {
        let id = RequestId::Integer(self.next_request_id);
        self.next_request_id += 1;
        let msg = JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: JsonRpcVersion,
            id: id.clone(),
            method: method.to_string(),
            params: Some(serde_json_to_msgpack_value(params)),
        });
        let bytes = rmp_serde::to_vec(&msg)?;
        self.ws.send(Message::Binary(bytes.into())).await?;
        Ok(id)
    }

    pub(crate) async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<()> {
        let msg = JSONRPCMessage::Notification(JSONRPCNotification {
            jsonrpc: JsonRpcVersion,
            method: method.to_string(),
            params: Some(serde_json_to_msgpack_value(params)),
        });
        let bytes = rmp_serde::to_vec(&msg)?;
        self.ws.send(Message::Binary(bytes.into())).await?;
        Ok(())
    }

    pub(crate) async fn send_raw_text(&mut self, text: &str) -> anyhow::Result<()> {
        self.ws.send(Message::Text(text.to_string().into())).await?;
        Ok(())
    }

    pub(crate) async fn next_event(&mut self) -> anyhow::Result<JSONRPCMessage> {
        self.next_event_with_timeout(EVENT_TIMEOUT).await
    }

    pub(crate) async fn wait_for_event<F>(
        &mut self,
        mut predicate: F,
    ) -> anyhow::Result<JSONRPCMessage>
    where
        F: FnMut(&JSONRPCMessage) -> bool,
    {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(anyhow!(
                    "timed out waiting for matching exec-server event after {EVENT_TIMEOUT:?}"
                ));
            }
            let remaining = deadline.duration_since(now);
            let event = self.next_event_with_timeout(remaining).await?;
            if predicate(&event) {
                return Ok(event);
            }
        }
    }

    pub(crate) async fn shutdown(&mut self) -> anyhow::Result<()> {
        self.child.start_kill()?;
        Ok(())
    }

    async fn next_event_with_timeout(
        &mut self,
        timeout_duration: Duration,
    ) -> anyhow::Result<JSONRPCMessage> {
        loop {
            let msg = timeout(timeout_duration, self.ws.next())
                .await
                .map_err(|_| anyhow!("timed out waiting for exec-server event"))?
                .ok_or_else(|| anyhow!("exec-server transport closed"))??;

            match msg {
                Message::Binary(bytes) => {
                    return Ok(rmp_serde::from_slice(&bytes)?);
                }
                Message::Close(_) => {
                    return Err(anyhow!("exec-server closed connection"));
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Text(text) => {
                    return Err(anyhow!("unexpected text frame: {text}"));
                }
                Message::Frame(_) => continue,
            }
        }
    }
}

fn serde_json_to_msgpack_value(v: serde_json::Value) -> rmpv::Value {
    let json_bytes = serde_json::to_vec(&v).unwrap_or_default();
    rmp_serde::from_slice(&json_bytes).unwrap_or(rmpv::Value::Nil)
}

async fn connect_websocket_when_ready(
    url: &str,
) -> anyhow::Result<WebSocketStream<tokio::net::TcpStream>> {
    let addr = url
        .strip_prefix("ws://")
        .ok_or_else(|| anyhow!("expected ws:// URL, got {url}"))?;

    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                let (ws, _) = tokio_tungstenite::client_async(format!("ws://{addr}"), stream)
                    .await
                    .map_err(|e| anyhow!("websocket handshake to {url}: {e}"))?;
                return Ok(ws);
            }
            Err(_) if Instant::now() < deadline => {
                sleep(CONNECT_RETRY_INTERVAL).await;
            }
            Err(err) => return Err(anyhow!("connect to {url}: {err}")),
        }
    }
}

async fn read_listen_url_from_stdout(child: &mut Child) -> anyhow::Result<String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture exec-server stdout"))?;
    let mut lines = BufReader::new(stdout).lines();
    let deadline = Instant::now() + CONNECT_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!(
                "timed out waiting for exec-server listen URL on stdout after {CONNECT_TIMEOUT:?}"
            ));
        }
        let remaining = deadline.duration_since(now);
        let line = timeout(remaining, lines.next_line())
            .await
            .map_err(|_| anyhow!("timed out waiting for exec-server stdout"))??
            .ok_or_else(|| anyhow!("exec-server stdout closed before emitting listen URL"))?;
        let listen_url = line.trim();
        if listen_url.starts_with("ws://") {
            return Ok(listen_url.to_string());
        }
    }
}
