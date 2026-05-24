//! Bidirectional WS + msgpack channel dispatch.
//!
//! Handles request/response correlation, method routing, and
//! notification forwarding over a single binary WebSocket stream.

use std::collections::HashMap;
use std::sync::Arc;

use nexal_utils_transport::agent::AgentMethod;
use nexal_utils_transport::notifications::{PROCESS_CLOSED, PROCESS_EXITED, PROCESS_OUTPUT};
use nexal_utils_transport::{JsonMessageConnection, JsonMessageConnectionEvent, to_msgpack_value};
use rmpv::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::process::events::ProcessEvent;
use crate::protocol::errors::{ChannelError, ChannelErrorKind};
use crate::protocol::wire::{
    ExecParams, FsCopyParams, FsCreateDirectoryParams, FsGetMetadataParams, FsReadDirectoryParams,
    FsReadFileParams, FsRemoveParams, FsWriteFileParams, InitializeParams, ProxyRegisterParams,
    ProxyUnregisterParams, ReadParams, TerminateParams, WriteParams,
};
use crate::server::services::ExecServerHandler;

/// Look up a string value by key in a map.
fn map_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))?
        .1
        .as_str()
}

/// Look up any value by key in a map.
fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

/// Build a msgpack map from key-value pairs.
fn mp_map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn mp_error(err: ChannelError) -> Value {
    let mut fields = vec![
        ("kind", Value::String(err.kind.to_string().into())),
        ("message", Value::String(err.message.into())),
    ];
    if let Some(d) = err.data {
        fields.push(("data", d));
    }
    Value::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

// ── MsgpackChannel: agent → gateway requests ───────────────────────

type PendingOutgoing = HashMap<u64, oneshot::Sender<Result<Value, String>>>;

/// Bidirectional channel handle for sending requests from the agent to
/// the gateway over the existing WebSocket connection.
pub(crate) struct MsgpackChannel {
    outgoing_tx: mpsc::Sender<Value>,
    pending: std::sync::Mutex<PendingOutgoing>,
    next_id: std::sync::Mutex<u64>,
}

impl MsgpackChannel {
    pub(crate) fn new(outgoing_tx: mpsc::Sender<Value>) -> Self {
        Self {
            outgoing_tx,
            pending: std::sync::Mutex::new(HashMap::new()),
            next_id: std::sync::Mutex::new(1),
        }
    }

    /// Send a request to the gateway and await the response.
    pub(crate) async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let v = *n;
            *n += 1;
            v
        };
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let request = mp_map(vec![
            ("id", Value::Integer(id.into())),
            ("method", Value::String(method.into())),
            ("params", params),
        ]);
        self.outgoing_tx
            .send(request)
            .await
            .map_err(|_| "connection closed".to_string())?;

        rx.await.map_err(|_| "connection closed".to_string())?
    }

    pub(crate) fn request_blocking(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let v = *n;
            *n += 1;
            v
        };
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let request = mp_map(vec![
            ("id", Value::Integer(id.into())),
            ("method", Value::String(method.into())),
            ("params", params),
        ]);
        if self.outgoing_tx.blocking_send(request).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err("connection closed".to_string());
        }

        rx.blocking_recv()
            .map_err(|_| "connection closed".to_string())?
    }

    /// Try to resolve a pending outgoing request. Returns true if the
    /// frame was a response to one of our requests.
    fn try_resolve_pending(&self, value: &Value) -> bool {
        if map_get(value, "method").is_some() {
            return false;
        }

        let Some(id_val) = map_get(value, "id") else {
            return false;
        };
        let Some(id) = id_val.as_u64() else {
            return false;
        };
        let Some(tx) = self.pending.lock().unwrap().remove(&id) else {
            return false;
        };
        if let Some(err) = map_get(value, "error") {
            let msg = map_get(err, "message")
                .and_then(Value::as_str)
                .unwrap_or("gateway error")
                .to_string();
            let _ = tx.send(Err(msg));
        } else {
            let result = map_get(value, "result").cloned().unwrap_or(Value::Nil);
            let _ = tx.send(Ok(result));
        }
        true
    }
}

/// Run the channel dispatch loop over a `JsonMessageConnection`.
///
/// Reads requests, dispatches to the handler, writes responses, and
/// forwards process event notifications. Returns when the connection
/// closes.
pub(crate) async fn run_dispatch(
    handler: Arc<ExecServerHandler>,
    outgoing_tx: mpsc::Sender<Value>,
    mut incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
) {
    let channel = Arc::new(MsgpackChannel::new(outgoing_tx.clone()));
    handler.set_channel(channel.clone());

    let notify_tx = outgoing_tx.clone();
    let handler_for_events = handler.clone();
    let events_task = tokio::spawn(async move {
        let mut rx = handler_for_events.subscribe_process_events();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let (method, params) = match &event {
                        ProcessEvent::OutputDelta(n) => {
                            (PROCESS_OUTPUT, to_msgpack_value(n).unwrap_or(Value::Nil))
                        }
                        ProcessEvent::Exited(n) => {
                            (PROCESS_EXITED, to_msgpack_value(n).unwrap_or(Value::Nil))
                        }
                        ProcessEvent::Closed(n) => {
                            (PROCESS_CLOSED, to_msgpack_value(n).unwrap_or(Value::Nil))
                        }
                    };
                    let notif = mp_map(vec![
                        ("method", Value::String(method.into())),
                        ("params", params),
                    ]);
                    if notify_tx.send(notif).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    while let Some(event) = incoming_rx.recv().await {
        match event {
            JsonMessageConnectionEvent::Message(value) => {
                // Check if this is a response to an outgoing request.
                if channel.try_resolve_pending(&value) {
                    continue;
                }

                // Otherwise it's an incoming request — dispatch it.
                let id = map_get(&value, "id").cloned();
                let method = map_str(&value, "method").unwrap_or("").to_string();
                let params = map_get(&value, "params").cloned().unwrap_or(Value::Nil);

                let params = unwrap_positional(params);

                let result = dispatch(&handler, &method, params).await;

                if let Some(req_id) = id {
                    let response = match result {
                        Ok(val) => mp_map(vec![("id", req_id), ("result", val)]),
                        Err(err) => mp_map(vec![("id", req_id), ("error", mp_error(err))]),
                    };
                    if outgoing_tx.send(response).await.is_err() {
                        break;
                    }
                }
            }
            JsonMessageConnectionEvent::MalformedMessage { reason } => {
                warn!("malformed message: {reason}");
                let response = mp_map(vec![
                    ("id", Value::Nil),
                    (
                        "error",
                        mp_error(ChannelError {
                            kind: ChannelErrorKind::Parse,
                            message: "parse error".into(),
                            data: Some(Value::String(reason.into())),
                        }),
                    ),
                ]);
                if outgoing_tx.send(response).await.is_err() {
                    break;
                }
            }
            JsonMessageConnectionEvent::Disconnected { reason } => {
                if let Some(reason) = reason {
                    debug!("client disconnected: {reason}");
                }
                break;
            }
        }
    }

    events_task.abort();
    handler.shutdown().await;
}

pub(crate) fn start_dispatch(
    handler: Arc<ExecServerHandler>,
    conn: JsonMessageConnection<Value>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let (outgoing_tx, incoming_rx, mut transport_tasks) = conn.into_parts();
    let dispatch_task = tokio::spawn(async move {
        run_dispatch(handler, outgoing_tx, incoming_rx).await;
    });
    transport_tasks.push(dispatch_task);
    transport_tasks
}

async fn dispatch(
    handler: &ExecServerHandler,
    method: &str,
    params: Value,
) -> Result<Value, ChannelError> {
    let Some(method) = AgentMethod::parse(method) else {
        return Err(ChannelError {
            kind: ChannelErrorKind::MethodNotFound,
            message: format!("method not found: {method}"),
            data: None,
        });
    };
    match method {
        AgentMethod::Initialize => {
            let _: InitializeParams = parse_params(params)?;
            let r = handler.initialize()?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::Initialized => {
            handler.initialized().map_err(internal_error)?;
            Ok(Value::Nil)
        }
        AgentMethod::ProcessStart => {
            let p: ExecParams = parse_params(params)?;
            let r = handler.exec(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::ProcessRead => {
            let p: ReadParams = parse_params(params)?;
            let r = handler.exec_read(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::ProcessWrite => {
            let p: WriteParams = parse_params(params)?;
            let r = handler.exec_write(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::ProcessTerminate => {
            let p: TerminateParams = parse_params(params)?;
            let r = handler.terminate(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsReadFile => {
            let p: FsReadFileParams = parse_params(params)?;
            let r = handler.fs_read_file(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsWriteFile => {
            let p: FsWriteFileParams = parse_params(params)?;
            let r = handler.fs_write_file(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsCreateDirectory => {
            let p: FsCreateDirectoryParams = parse_params(params)?;
            let r = handler.fs_create_directory(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsGetMetadata => {
            let p: FsGetMetadataParams = parse_params(params)?;
            let r = handler.fs_get_metadata(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsReadDirectory => {
            let p: FsReadDirectoryParams = parse_params(params)?;
            let r = handler.fs_read_directory(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsRemove => {
            let p: FsRemoveParams = parse_params(params)?;
            let r = handler.fs_remove(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::FsCopy => {
            let p: FsCopyParams = parse_params(params)?;
            let r = handler.fs_copy(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::ProxyRegister => {
            let p: ProxyRegisterParams = parse_params(params)?;
            let r = handler.proxy_register(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
        AgentMethod::ProxyUnregister => {
            let p: ProxyUnregisterParams = parse_params(params)?;
            let r = handler.proxy_unregister(p).await?;
            Ok(to_msgpack_value(&r).unwrap_or(Value::Nil))
        }
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, ChannelError> {
    rmpv::ext::from_value(params).map_err(|err| ChannelError {
        kind: ChannelErrorKind::InvalidParams,
        message: format!("invalid params: {err}"),
        data: None,
    })
}

fn internal_error(msg: String) -> ChannelError {
    ChannelError {
        kind: ChannelErrorKind::Internal,
        message: msg,
        data: None,
    }
}

/// Unwrap positional params.
/// `[{...}]` → `{...}`, `[]` → `null`, anything else → passthrough.
fn unwrap_positional(params: Value) -> Value {
    match params {
        Value::Array(mut arr) if arr.len() == 1 => arr.remove(0),
        Value::Array(arr) if arr.is_empty() => Value::Nil,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwrap_single_element_array() {
        let input = Value::Array(vec![mp_map(vec![("name", Value::String("test".into()))])]);
        let expected = mp_map(vec![("name", Value::String("test".into()))]);
        assert_eq!(unwrap_positional(input), expected);
    }

    #[test]
    fn unwrap_empty_array() {
        assert_eq!(unwrap_positional(Value::Array(vec![])), Value::Nil);
    }

    #[test]
    fn passthrough_object() {
        let input = mp_map(vec![("name", Value::String("test".into()))]);
        assert_eq!(unwrap_positional(input.clone()), input);
    }

    #[test]
    fn passthrough_null() {
        assert_eq!(unwrap_positional(Value::Nil), Value::Nil);
    }
}
