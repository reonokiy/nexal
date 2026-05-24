#![allow(dead_code)]

use std::process::Stdio;
use std::time::Duration;

use anyhow::anyhow;
use futures::{SinkExt, StreamExt};
use nexal_utils_cargo_bin::cargo_bin;
use rmpv::Value;
use serde::Serialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::Instant;
use tokio::time::sleep;
use tokio::time::timeout;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ExecServerHarness {
    child: Child,
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
        ws,
        next_request_id: 1,
    })
}

fn to_msgpack(v: &serde_json::Value) -> Value {
    let mut buf = Vec::new();
    let mut ser = rmp_serde::Serializer::new(&mut buf).with_struct_map();
    v.serialize(&mut ser).unwrap();
    rmp_serde::from_slice(&buf).unwrap_or(Value::Nil)
}

fn mp_map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

impl ExecServerHarness {
    pub(crate) async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<i64> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let msg = mp_map(vec![
            ("id", Value::Integer(id.into())),
            ("method", Value::String(method.into())),
            ("params", to_msgpack(&params)),
        ]);
        let bytes = rmp_serde::to_vec(&msg)?;
        self.ws.send(Message::Binary(bytes.into())).await?;
        Ok(id)
    }

    pub(crate) async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<()> {
        let msg = mp_map(vec![
            ("method", Value::String(method.into())),
            ("params", to_msgpack(&params)),
        ]);
        let bytes = rmp_serde::to_vec(&msg)?;
        self.ws.send(Message::Binary(bytes.into())).await?;
        Ok(())
    }

    pub(crate) async fn next_event(&mut self) -> anyhow::Result<Value> {
        self.next_event_with_timeout(EVENT_TIMEOUT).await
    }

    pub(crate) async fn wait_for_event<F>(&mut self, mut predicate: F) -> anyhow::Result<Value>
    where
        F: FnMut(&Value) -> bool,
    {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(anyhow!(
                    "timed out waiting for matching event after {EVENT_TIMEOUT:?}"
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
    ) -> anyhow::Result<Value> {
        loop {
            let msg = timeout(timeout_duration, self.ws.next())
                .await
                .map_err(|_| anyhow!("timed out waiting for event"))?
                .ok_or_else(|| anyhow!("transport closed"))??;

            match msg {
                Message::Binary(bytes) => {
                    return Ok(rmp_serde::from_slice(&bytes)?);
                }
                Message::Close(_) => return Err(anyhow!("closed")),
                Message::Ping(_) | Message::Pong(_) => continue,
                other => return Err(anyhow!("unexpected msg: {other:?}")),
            }
        }
    }
}

fn map_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))?
        .1
        .as_str()
}

fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

pub(crate) fn event_id(event: &Value) -> Option<i64> {
    map_get(event, "id").and_then(|v| v.as_i64())
}

pub(crate) fn event_has_key(event: &Value, key: &str) -> bool {
    map_get(event, key).is_some()
}

pub(crate) fn event_get<'a>(event: &'a Value, key: &str) -> Option<&'a Value> {
    map_get(event, key)
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
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut lines = BufReader::new(stdout).lines();
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!(
                "timed out waiting for listen URL after {CONNECT_TIMEOUT:?}"
            ));
        }
        let remaining = deadline.duration_since(now);
        let line = timeout(remaining, lines.next_line())
            .await
            .map_err(|_| anyhow!("timed out waiting for stdout"))??
            .ok_or_else(|| anyhow!("stdout closed before listen URL"))?;
        let listen_url = line.trim();
        if listen_url.starts_with("ws://") {
            return Ok(listen_url.to_string());
        }
    }
}
