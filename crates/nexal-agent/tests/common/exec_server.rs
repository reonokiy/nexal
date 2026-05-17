#![allow(dead_code)]

use std::process::Stdio;
use std::time::Duration;

use anyhow::anyhow;
use nexal_agent::JSONRPCMessage;
use nexal_agent::JSONRPCNotification;
use nexal_agent::JSONRPCRequest;
use nexal_agent::RequestId;
use nexal_utils_cargo_bin::cargo_bin;
use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::Value;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::sleep;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ExecServerHarness {
    child: Child,
    #[allow(dead_code)]
    url: String,
    write_tx: mpsc::Sender<Value>,
    incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
    _transport_tasks: Vec<tokio::task::JoinHandle<()>>,
    next_request_id: i64,
}

impl Drop for ExecServerHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        for task in &self._transport_tasks {
            task.abort();
        }
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
    let (write_tx, incoming_rx, transport_tasks) =
        connect_websocket_when_ready(&url).await?;

    Ok(ExecServerHarness {
        child,
        url,
        write_tx,
        incoming_rx,
        _transport_tasks: transport_tasks,
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
        self.send_message(JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: jsonrpsee::types::TwoPointZero,
            id: id.clone(),
            method: method.to_string(),
            params: Some(params),
        }))
        .await?;
        Ok(id)
    }

    pub(crate) async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<()> {
        self.send_message(JSONRPCMessage::Notification(JSONRPCNotification {
            jsonrpc: jsonrpsee::types::TwoPointZero,
            method: method.to_string(),
            params: Some(params),
        }))
        .await
    }

    pub(crate) async fn send_raw_text(&mut self, text: &str) -> anyhow::Result<()> {
        let value: Value = serde_json::from_str(text)
            .unwrap_or_else(|_| Value::String(text.to_string()));
        self.write_tx
            .send(value)
            .await
            .map_err(|_| anyhow!("transport closed"))?;
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

    async fn send_message(&mut self, message: JSONRPCMessage) -> anyhow::Result<()> {
        let value = serde_json::to_value(&message)?;
        self.write_tx
            .send(value)
            .await
            .map_err(|_| anyhow!("transport closed"))?;
        Ok(())
    }

    async fn next_event_with_timeout(
        &mut self,
        timeout_duration: Duration,
    ) -> anyhow::Result<JSONRPCMessage> {
        loop {
            let event = timeout(timeout_duration, self.incoming_rx.recv())
                .await
                .map_err(|_| anyhow!("timed out waiting for exec-server event"))?
                .ok_or_else(|| anyhow!("exec-server transport closed"))?;

            match event {
                JsonMessageConnectionEvent::Message(value) => {
                    return Ok(serde_json::from_value(value)?);
                }
                JsonMessageConnectionEvent::MalformedMessage { reason } => {
                    return Err(anyhow!("malformed message: {reason}"));
                }
                JsonMessageConnectionEvent::Disconnected { reason } => {
                    return Err(anyhow!(
                        "exec-server disconnected: {}",
                        reason.unwrap_or_else(|| "unknown".to_string())
                    ));
                }
            }
        }
    }
}

async fn connect_websocket_when_ready(
    url: &str,
) -> anyhow::Result<(
    mpsc::Sender<Value>,
    mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
    Vec<tokio::task::JoinHandle<()>>,
)> {
    let addr = url
        .strip_prefix("ws://")
        .ok_or_else(|| anyhow!("expected ws:// URL, got {url}"))?;

    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                let ws_stream = match tokio_tungstenite::client_async(format!("ws://{addr}"), stream).await {
                    Ok((ws, _)) => ws,
                    Err(e) => return Err(anyhow!("websocket handshake to {url}: {e}")),
                };
                let conn = JsonMessageConnection::<Value>::from_websocket(
                    ws_stream,
                    format!("test-client {url}"),
                );
                return Ok(conn.into_parts());
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
