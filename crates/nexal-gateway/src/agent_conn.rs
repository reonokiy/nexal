//! `AgentConn` — one WebSocket connection between the gateway and a
//! single in-container `nexal-agent`.
//!
//! Lifecycle:
//!   1. `AgentConn::connect(url)` — open WebSocket, do `initialize` +
//!      `initialized` handshake.
//!   2. `invoke(method, params)` — sends a JSON-RPC request, awaits
//!      the matching response. Allocates its own request ids
//!      independent from the frontend's ids.
//!   3. Notifications received from the agent are forwarded into a
//!      provided `mpsc::Sender<AgentNotification>` so the gateway can
//!      relay them to the frontend wrapped as `agent/notify`.
//!   4. `close()` — drops the connection; the background reader exits
//!      and pending invocations resolve with `Closed`.

use std::collections::HashMap;
use std::sync::Arc;

use nexal_utils_transport::agent::AgentMethod;
use nexal_utils_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use rmpv::Value;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::warn;

use crate::protocol::JsonRpcError;

#[derive(Debug, Clone, Error)]
pub enum AgentConnError {
    #[error("connect to agent failed: {0}")]
    Connect(String),
    #[error("agent send failed: {0}")]
    Send(String),
    #[error("agent connection closed")]
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

/// Handler for requests initiated by the agent (e.g. skills/list,
/// skills/read). Takes method + params, returns a result or error.
pub type AgentRequestHandler = Arc<
    dyn Fn(
            String,
            Option<Value>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send>>
        + Send
        + Sync,
>;

type Pending = HashMap<u64, oneshot::Sender<Result<Value, AgentConnError>>>;

pub struct AgentConn {
    write_tx: mpsc::Sender<Value>,
    pending: Arc<Mutex<Pending>>,
    next_id: Arc<Mutex<u64>>,
    closed: Arc<Mutex<bool>>,
    reader: tokio::task::JoinHandle<()>,
    transport_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl AgentConn {
    pub async fn connect(
        url: &str,
        client_name: &str,
        notify_tx: mpsc::Sender<AgentNotification>,
        request_handler: Option<AgentRequestHandler>,
    ) -> Result<Self, AgentConnError> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| AgentConnError::Connect(format!("connect {url}: {e}")))?;

        let conn = JsonMessageConnection::<Value>::from_websocket_binary(
            ws_stream,
            format!("agent ws {url}"),
        );

        let (write_tx, incoming_rx, transport_tasks) = conn.into_parts();

        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));
        let closed = Arc::new(Mutex::new(false));

        let pending_for_reader = pending.clone();
        let closed_for_reader = closed.clone();
        let write_tx_for_reader = write_tx.clone();

        let reader = tokio::spawn(async move {
            run_reader(
                incoming_rx,
                &pending_for_reader,
                &notify_tx,
                request_handler.as_ref(),
                &write_tx_for_reader,
            )
            .await;
            *closed_for_reader.lock().await = true;
            drain_pending(&pending_for_reader).await;
        });

        let agent_conn = Self {
            write_tx,
            pending,
            next_id,
            closed,
            reader,
            transport_tasks,
        };

        let _init: Value = agent_conn
            .invoke(
                AgentMethod::Initialize.as_str(),
                Some(msgpack_map_str(&[("client_name", client_name)])),
            )
            .await?;
        let _ = agent_conn
            .invoke(AgentMethod::Initialized.as_str(), None)
            .await?;
        Ok(agent_conn)
    }

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
        self.write_tx
            .send(build_rpc_request(id, method, params.unwrap_or(Value::Nil)))
            .await
            .map_err(|_| AgentConnError::Closed)?;
        match rx.await {
            Ok(res) => res,
            Err(_) => Err(AgentConnError::Closed),
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AgentConnError> {
        self.write_tx
            .send(build_rpc_notification(method, params))
            .await
            .map_err(|_| AgentConnError::Closed)
    }

    pub async fn close(&self) {
        *self.closed.lock().await = true;
        self.reader.abort();
        for task in &self.transport_tasks {
            task.abort();
        }
        drain_pending(&self.pending).await;
    }
}

async fn run_reader(
    mut incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
    pending: &Arc<Mutex<Pending>>,
    notify_tx: &mpsc::Sender<AgentNotification>,
    request_handler: Option<&AgentRequestHandler>,
    write_tx: &mpsc::Sender<Value>,
) {
    while let Some(event) = incoming_rx.recv().await {
        match event {
            JsonMessageConnectionEvent::Message(value) => {
                if let Err(err) =
                    dispatch_frame(value, pending, notify_tx, request_handler, write_tx).await
                {
                    warn!("agent frame dispatch error: {err}");
                }
            }
            JsonMessageConnectionEvent::MalformedMessage { reason } => {
                warn!("agent frame malformed: {reason}");
            }
            JsonMessageConnectionEvent::Disconnected { reason } => {
                if let Some(reason) = reason {
                    warn!("agent disconnected: {reason}");
                }
                break;
            }
        }
    }
}

async fn dispatch_frame(
    value: Value,
    pending: &Arc<Mutex<Pending>>,
    notify_tx: &mpsc::Sender<AgentNotification>,
    request_handler: Option<&AgentRequestHandler>,
    write_tx: &mpsc::Sender<Value>,
) -> Result<(), AgentConnError> {
    // If this frame has an ID but no method, check if it's a response to
    // one of our pending requests first. Frames with both ID and method
    // are agent-initiated requests and must not be consumed as responses.
    if map_get(&value, "method").is_none()
        && let Some(id_val) = map_get(&value, "id")
        && let Some(id) = id_val.as_u64()
    {
        let mut map = pending.lock().await;
        if let Some(tx) = map.remove(&id) {
            if let Some(err) = map_get(&value, "error") {
                let code = agent_error_code(err);
                let msg = map_get(err, "message")
                    .and_then(Value::as_str)
                    .unwrap_or("agent error")
                    .to_string();
                let _ = tx.send(Err(AgentConnError::AgentError { code, message: msg }));
            } else {
                let result = map_get(&value, "result").cloned().unwrap_or(Value::Nil);
                let _ = tx.send(Ok(result));
            }
            return Ok(());
        }
    }

    // Agent-initiated request. Dispatch it through the handler and send back a response.
    if let Some(id_val) = map_get(&value, "id") {
        if let Some(handler) = request_handler {
            let id = id_val.clone();
            let method = map_get(&value, "method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let params = map_get(&value, "params").cloned();

            let handler = handler.clone();
            let write_tx = write_tx.clone();
            tokio::spawn(async move {
                let result = handler(method, params).await;
                let response = match result {
                    Ok(val) => Value::Map(vec![
                        (Value::String("id".into()), id),
                        (Value::String("result".into()), val),
                    ]),
                    Err(msg) => Value::Map(vec![
                        (Value::String("id".into()), id),
                        (
                            Value::String("error".into()),
                            Value::Map(vec![
                                (
                                    Value::String("code".into()),
                                    Value::Integer((-32603i64).into()),
                                ),
                                (Value::String("message".into()), Value::String(msg.into())),
                            ]),
                        ),
                    ]),
                };
                let _ = write_tx.send(response).await;
            });
            return Ok(());
        }
    }

    // No ID or no handler — treat as notification.
    let method = map_get(&value, "method")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentConnError::BadFrame("notification missing method".into()))?
        .to_string();
    let params = map_get(&value, "params").cloned();
    let _ = notify_tx.send(AgentNotification { method, params }).await;
    Ok(())
}

async fn drain_pending(pending: &Arc<Mutex<Pending>>) {
    let mut pending = pending.lock().await;
    for (_id, tx) in pending.drain() {
        let _ = tx.send(Err(AgentConnError::Closed));
    }
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

fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

fn agent_error_code(err: &Value) -> i32 {
    if let Some(code) = map_get(err, "code").and_then(Value::as_i64) {
        return code as i32;
    }

    match map_get(err, "kind").and_then(Value::as_str) {
        Some("parse") => crate::protocol::error_code::PARSE_ERROR,
        Some("invalid_request") => crate::protocol::error_code::INVALID_REQUEST,
        Some("method_not_found") => crate::protocol::error_code::METHOD_NOT_FOUND,
        Some("invalid_params") => crate::protocol::error_code::INVALID_PARAMS,
        Some("internal") | None => crate::protocol::error_code::INTERNAL_ERROR,
        Some(_) => crate::protocol::error_code::INTERNAL_ERROR,
    }
}

fn msgpack_map_str(pairs: &[(&str, &str)]) -> Value {
    Value::Map(
        pairs
            .iter()
            .map(|(k, v)| (Value::String((*k).into()), Value::String((*v).into())))
            .collect(),
    )
}

fn build_rpc_request(id: u64, method: &str, params: Value) -> Value {
    Value::Map(vec![
        (Value::String("id".into()), Value::Integer(id.into())),
        (Value::String("method".into()), Value::String(method.into())),
        (Value::String("params".into()), params),
    ])
}

fn build_rpc_notification(method: &str, params: Value) -> Value {
    Value::Map(vec![
        (Value::String("method".into()), Value::String(method.into())),
        (Value::String("params".into()), params),
    ])
}

#[cfg(test)]
mod tests {
    use super::AgentConnError;
    use crate::protocol::JsonRpcError;

    #[test]
    fn agent_error_preserves_code_and_message() {
        let err = AgentConnError::AgentError {
            code: -32042,
            message: "bad param".into(),
        };
        let rpc: JsonRpcError = err.into();
        assert_eq!(rpc.code, -32042);
        assert_eq!(rpc.message, "bad param");
    }

    #[test]
    fn closed_maps_to_neg32000() {
        let rpc: JsonRpcError = AgentConnError::Closed.into();
        assert_eq!(rpc.code, -32000);
        assert!(rpc.message.contains("closed"));
    }

    #[test]
    fn connect_error_maps_to_neg32020() {
        let rpc: JsonRpcError = AgentConnError::Connect("timeout".into()).into();
        assert_eq!(rpc.code, -32020);
        assert!(rpc.message.contains("timeout"));
    }

    #[test]
    fn send_error_maps_to_neg32020() {
        let rpc: JsonRpcError = AgentConnError::Send("broken pipe".into()).into();
        assert_eq!(rpc.code, -32020);
        assert!(rpc.message.contains("broken pipe"));
    }

    #[test]
    fn bad_frame_maps_to_neg32603() {
        let rpc: JsonRpcError = AgentConnError::BadFrame("not json".into()).into();
        assert_eq!(rpc.code, -32603);
        assert!(rpc.message.contains("not json"));
    }
}
