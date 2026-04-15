//! `AgentConn` — one WebSocket connection between the gateway and a
//! single in-container `nexal-agent`.
//!
//! Lifecycle:
//!   1. `AgentConn::connect(ws_url)` — open WS, do `initialize` +
//!      `initialized` handshake.
//!   2. `invoke(method, params)` — sends a JSON-RPC request, awaits
//!      the matching response. Allocates its own request ids
//!      independent from the frontend's ids.
//!   3. Notifications received from the agent are forwarded into a
//!      provided `mpsc::Sender<AgentNotification>` so the gateway can
//!      relay them to the frontend wrapped as `agent/notify`.
//!   4. `close()` — drops the WS; the background reader exits and
//!      pending invocations resolve with `Closed`.

use std::collections::HashMap;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{
    connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, warn};

use crate::protocol::{JsonRpcError, JSONRPC_VERSION};

#[derive(Debug, Clone, Error)]
pub enum AgentConnError {
    #[error("connect to agent failed: {0}")]
    Connect(String),
    #[error("agent ws send failed: {0}")]
    Send(String),
    #[error("agent ws closed")]
    Closed,
    #[error("agent returned error {code}: {message}")]
    AgentError { code: i32, message: String },
    #[error("invalid agent frame: {0}")]
    BadFrame(String),
}

pub struct AgentNotification {
    pub method: String,
    pub params: Option<Value>,
}

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Pending = HashMap<u64, oneshot::Sender<Result<Value, AgentConnError>>>;

pub struct AgentConn {
    sink: Arc<Mutex<futures::stream::SplitSink<WsStream, Message>>>,
    pending: Arc<Mutex<Pending>>,
    next_id: Arc<Mutex<u64>>,
    /// Closed → reader task ended, all future invokes will error.
    closed: Arc<Mutex<bool>>,
    _reader: tokio::task::JoinHandle<()>,
}

impl AgentConn {
    pub async fn connect(
        ws_url: &str,
        client_name: &str,
        notify_tx: mpsc::Sender<AgentNotification>,
    ) -> Result<Self, AgentConnError> {
        let (ws, _resp) = connect_async(ws_url)
            .await
            .map_err(|e| AgentConnError::Connect(format!("{e}")))?;
        let (sink, mut stream) = ws.split();

        let sink = Arc::new(Mutex::new(sink));
        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));
        let closed = Arc::new(Mutex::new(false));

        let pending_for_reader = pending.clone();
        let closed_for_reader = closed.clone();

        let reader = tokio::spawn(async move {
            while let Some(frame) = stream.next().await {
                match frame {
                    Ok(Message::Text(text)) => {
                        if let Err(err) =
                            dispatch_frame(&text, &pending_for_reader, &notify_tx).await
                        {
                            warn!("agent frame dispatch error: {err}");
                        }
                    }
                    Ok(Message::Binary(bytes)) => match std::str::from_utf8(&bytes) {
                        Ok(text) => {
                            if let Err(err) =
                                dispatch_frame(text, &pending_for_reader, &notify_tx).await
                            {
                                warn!("agent binary frame dispatch error: {err}");
                            }
                        }
                        Err(_) => warn!("agent sent non-utf8 binary frame, dropping"),
                    },
                    Ok(Message::Close(_)) => {
                        debug!("agent ws close frame");
                        break;
                    }
                    Ok(_) => {} // pings/pongs handled by tungstenite
                    Err(err) => {
                        warn!("agent ws read error: {err}");
                        break;
                    }
                }
            }
            *closed_for_reader.lock().await = true;
            // Fail every pending request — gateway frontend gets a clean error.
            let mut pend = pending_for_reader.lock().await;
            for (_id, tx) in pend.drain() {
                let _ = tx.send(Err(AgentConnError::Closed));
            }
        });

        let conn = Self {
            sink,
            pending,
            next_id,
            closed,
            _reader: reader,
        };

        // LSP-style handshake — must complete before any other call.
        let _init: Value = conn
            .invoke("initialize", Some(json!({ "clientName": client_name })))
            .await?;
        conn.notify("initialized", json!({})).await?;
        Ok(conn)
    }

    /// Send a request, await response.
    pub async fn invoke(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, AgentConnError> {
        if *self.closed.lock().await {
            return Err(AgentConnError::Closed);
        }
        let id = {
            let mut n = self.next_id.lock().await;
            let v = *n;
            *n += 1;
            v
        };
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let frame = json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": id,
            "method": method,
            "params": params.unwrap_or(Value::Null),
        });
        let text = serde_json::to_string(&frame)
            .map_err(|e| AgentConnError::Send(format!("encode: {e}")))?;
        self.sink
            .lock()
            .await
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| AgentConnError::Send(format!("{e}")))?;
        match rx.await {
            Ok(res) => res,
            Err(_) => Err(AgentConnError::Closed),
        }
    }

    /// Send a notification (fire-and-forget).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AgentConnError> {
        let frame = json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": method,
            "params": params,
        });
        let text = serde_json::to_string(&frame)
            .map_err(|e| AgentConnError::Send(format!("encode: {e}")))?;
        self.sink
            .lock()
            .await
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| AgentConnError::Send(format!("{e}")))?;
        Ok(())
    }

    pub async fn close(&self) {
        *self.closed.lock().await = true;
        let _ = self.sink.lock().await.close().await;
    }
}

async fn dispatch_frame(
    text: &str,
    pending: &Arc<Mutex<Pending>>,
    notify_tx: &mpsc::Sender<AgentNotification>,
) -> Result<(), AgentConnError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| AgentConnError::BadFrame(format!("json: {e}")))?;
    if let Some(id_val) = value.get("id") {
        if let Some(id) = id_val.as_u64() {
            let mut map = pending.lock().await;
            if let Some(tx) = map.remove(&id) {
                if let Some(err) = value.get("error") {
                    let code = err.get("code").and_then(Value::as_i64).unwrap_or(-32603) as i32;
                    let msg = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("agent error")
                        .to_string();
                    let _ = tx.send(Err(AgentConnError::AgentError { code, message: msg }));
                } else {
                    let result = value.get("result").cloned().unwrap_or(Value::Null);
                    let _ = tx.send(Ok(result));
                }
            }
            return Ok(());
        }
    }
    // Notification (no id, or id with non-numeric value).
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentConnError::BadFrame("notification missing method".into()))?
        .to_string();
    let params = value.get("params").cloned();
    let _ = notify_tx.send(AgentNotification { method, params }).await;
    Ok(())
}

impl From<AgentConnError> for JsonRpcError {
    fn from(e: AgentConnError) -> Self {
        let (code, message) = match &e {
            AgentConnError::AgentError { code, message } => (*code, message.clone()),
            AgentConnError::Closed => (-32000, "agent connection closed".into()),
            AgentConnError::Connect(m) => (-32020, format!("agent connect: {m}")),
            AgentConnError::Send(m) => (-32020, format!("agent send: {m}")),
            AgentConnError::BadFrame(m) => (-32603, format!("agent frame: {m}")),
        };
        JsonRpcError {
            code,
            message,
            data: None,
        }
    }
}
