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

use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tracing::warn;

use crate::protocol::{JSONRPC_VERSION, JsonRpcError};

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

type Pending = HashMap<u64, oneshot::Sender<Result<Value, AgentConnError>>>;

pub struct AgentConn {
    write_tx: mpsc::Sender<Value>,
    pending: Arc<Mutex<Pending>>,
    next_id: Arc<Mutex<u64>>,
    /// Closed → reader task ended, all future invokes will error.
    closed: Arc<Mutex<bool>>,
    reader: tokio::task::JoinHandle<()>,
    transport_tasks: Vec<tokio::task::JoinHandle<()>>,
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
        let (write_tx, mut incoming_rx, transport_tasks) =
            JsonMessageConnection::from_websocket(ws, format!("agent websocket {ws_url}"))
                .into_parts();

        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));
        let closed = Arc::new(Mutex::new(false));

        let pending_for_reader = pending.clone();
        let closed_for_reader = closed.clone();

        let reader = tokio::spawn(async move {
            while let Some(event) = incoming_rx.recv().await {
                match event {
                    JsonMessageConnectionEvent::Message(value) => {
                        if let Err(err) =
                            dispatch_frame(value, &pending_for_reader, &notify_tx).await
                        {
                            warn!("agent frame dispatch error: {err}");
                        }
                    }
                    JsonMessageConnectionEvent::MalformedMessage { reason } => {
                        warn!("agent frame dispatch error: {reason}");
                    }
                    JsonMessageConnectionEvent::Disconnected { reason } => {
                        if let Some(reason) = reason {
                            warn!("agent ws read error: {reason}");
                        }
                        break;
                    }
                }
            }
            *closed_for_reader.lock().await = true;
            drain_pending(&pending_for_reader).await;
        });

        let conn = Self {
            write_tx,
            pending,
            next_id,
            closed,
            reader,
            transport_tasks,
        };

        let _init: Value = conn
            .invoke("initialize", Some(json!({ "client_name": client_name })))
            .await?;
        // `initialized` is declared as a no-arg METHOD in the jsonrpsee
        // server (not a JSON-RPC notification), so send it as a request
        // with an id and wait for the response.
        let _ = conn.invoke("initialized", None).await?;
        Ok(conn)
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
            .send(json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": id,
                "method": method,
                "params": wrap_params_positional(params),
            }))
            .await
            .map_err(|_| AgentConnError::Closed)?;
        match rx.await {
            Ok(res) => res,
            Err(_) => Err(AgentConnError::Closed),
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AgentConnError> {
        self.write_tx
            .send(json!({
                "jsonrpc": JSONRPC_VERSION,
                "method": method,
                "params": wrap_params_positional(Some(params)),
            }))
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

async fn dispatch_frame(
    value: Value,
    pending: &Arc<Mutex<Pending>>,
    notify_tx: &mpsc::Sender<AgentNotification>,
) -> Result<(), AgentConnError> {
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

    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentConnError::BadFrame("notification missing method".into()))?
        .to_string();
    let params = value.get("params").cloned();
    let _ = notify_tx.send(AgentNotification { method, params }).await;
    Ok(())
}

async fn drain_pending(pending: &Arc<Mutex<Pending>>) {
    let mut pending = pending.lock().await;
    for (_id, tx) in pending.drain() {
        let _ = tx.send(Err(AgentConnError::Closed));
    }
}

/// nexal-agent uses jsonrpsee, which serializes single-parameter
/// methods as a positional array (`"params": [{...}]`). Frontend and
/// our own internal callers pass a single object — wrap it before
/// sending.
///
/// - `None` → `[]` (matches a zero-arg jsonrpsee method).
/// - `Some(Value::Array(_))` → passed through unchanged (caller
///   already prepared positional form, e.g. for multi-param methods).
/// - `Some(Value::Null)` → `[]`.
/// - anything else → `[value]` (single-arg wrap).
fn wrap_params_positional(params: Option<Value>) -> Value {
    match params {
        None | Some(Value::Null) => Value::Array(Vec::new()),
        Some(Value::Array(arr)) => Value::Array(arr),
        // Empty `{}` means "no args" — common idiom on the Bun side.
        Some(Value::Object(obj)) if obj.is_empty() => Value::Array(Vec::new()),
        Some(other) => Value::Array(vec![other]),
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_params_positional;
    use serde_json::{json, Value};

    #[test]
    fn none_becomes_empty_array() {
        assert_eq!(wrap_params_positional(None), json!([]));
    }

    #[test]
    fn null_becomes_empty_array() {
        assert_eq!(wrap_params_positional(Some(Value::Null)), json!([]));
    }

    #[test]
    fn empty_object_becomes_empty_array() {
        assert_eq!(wrap_params_positional(Some(json!({}))), json!([]));
    }

    #[test]
    fn array_passes_through_unchanged() {
        let arr = json!([{"a": 1}, 2, "three"]);
        assert_eq!(wrap_params_positional(Some(arr.clone())), arr);
    }

    #[test]
    fn single_object_wraps_in_array() {
        assert_eq!(
            wrap_params_positional(Some(json!({"client_name": "x"}))),
            json!([{"client_name": "x"}])
        );
    }

    #[test]
    fn scalar_wraps_in_array() {
        assert_eq!(wrap_params_positional(Some(json!(42))), json!([42]));
        assert_eq!(wrap_params_positional(Some(json!("hi"))), json!(["hi"]));
        assert_eq!(wrap_params_positional(Some(json!(true))), json!([true]));
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
