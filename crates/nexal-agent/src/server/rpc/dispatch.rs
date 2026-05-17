//! JSON-RPC dispatch over WebSocket (MessagePack binary frames).
//!
//! Manual method dispatch over a MessagePack binary stream via
//! `JsonMessageConnection`. Handles serialization, method routing,
//! and notification forwarding to connected clients.

use std::sync::Arc;

use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use rmpv::Value;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::transport::protocol::{
    EXEC_CLOSED_METHOD, EXEC_EXITED_METHOD, EXEC_OUTPUT_DELTA_METHOD, ExecParams, FsCopyParams,
    FsCreateDirectoryParams, FsGetMetadataParams, FsMethod, FsReadDirectoryParams, FsReadFileParams,
    FsRemoveParams, FsWriteFileParams, InitializeParams, JSONRPCErrorError, LifecycleMethod, Method,
    ProcessMethod, ProxyMethod, ProxyRegisterParams, ProxyUnregisterParams, ReadParams,
    TerminateParams, WriteParams, parse_method, ERROR_CODE_METHOD_NOT_FOUND,
    ERROR_CODE_INVALID_PARAMS, ERROR_CODE_INTERNAL, ERROR_CODE_PARSE,
};
use crate::server::services::{ExecServerHandler, ProcessEvent};

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

fn mp_error(code: i64, message: &str, data: Option<Value>) -> Value {
    let mut fields = vec![
        ("code", Value::Integer(code.into())),
        ("message", Value::String(message.into())),
    ];
    if let Some(d) = data {
        fields.push(("data", d));
    }
    Value::Map(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

/// Run the JSON-RPC dispatch loop over a `JsonMessageConnection`.
///
/// Reads requests, dispatches to the handler, writes responses, and
/// forwards process event notifications. Returns when the connection
/// closes.
pub(crate) async fn run_dispatch(
    handler: Arc<ExecServerHandler>,
    outgoing_tx: mpsc::Sender<Value>,
    mut incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
) {
    let notify_tx = outgoing_tx.clone();
    let handler_for_events = handler.clone();
    let events_task = tokio::spawn(async move {
        let mut rx = handler_for_events.subscribe_process_events();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let (method, params) = match &event {
                        ProcessEvent::OutputDelta(n) => (
                            EXEC_OUTPUT_DELTA_METHOD,
                            rmpv::ext::to_value(n).unwrap_or(Value::Nil),
                        ),
                        ProcessEvent::Exited(n) => (
                            EXEC_EXITED_METHOD,
                            rmpv::ext::to_value(n).unwrap_or(Value::Nil),
                        ),
                        ProcessEvent::Closed(n) => (
                            EXEC_CLOSED_METHOD,
                            rmpv::ext::to_value(n).unwrap_or(Value::Nil),
                        ),
                    };
                    let notif = mp_map(vec![
                        ("jsonrpc", Value::String("2.0".into())),
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
                let id = map_get(&value, "id").cloned();
                let method = map_str(&value, "method")
                    .unwrap_or("")
                    .to_string();
                let params = map_get(&value, "params").cloned().unwrap_or(Value::Nil);

                let params = unwrap_positional(params);

                let result = dispatch(&handler, &method, params).await;

                if let Some(req_id) = id {
                    let response = match result {
                        Ok(val) => mp_map(vec![
                            ("jsonrpc", Value::String("2.0".into())),
                            ("id", req_id),
                            ("result", val),
                        ]),
                        Err(err) => mp_map(vec![
                            ("jsonrpc", Value::String("2.0".into())),
                            ("id", req_id),
                            ("error", mp_error(err.code, &err.message, err.data)),
                        ]),
                    };
                    if outgoing_tx.send(response).await.is_err() {
                        break;
                    }
                }
            }
            JsonMessageConnectionEvent::MalformedMessage { reason } => {
                warn!("malformed message: {reason}");
                let response = mp_map(vec![
                    ("jsonrpc", Value::String("2.0".into())),
                    ("id", Value::Nil),
                    ("error", mp_error(ERROR_CODE_PARSE, "Parse error", Some(Value::String(reason.into())))),
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
) -> Result<Value, JSONRPCErrorError> {
    let Some(method) = parse_method(method) else {
        return Err(JSONRPCErrorError {
            code: ERROR_CODE_METHOD_NOT_FOUND,
            message: format!("method not found: {method}"),
            data: None,
        });
    };
    match method {
        Method::Lifecycle(m) => match m {
            LifecycleMethod::Initialize => {
                let _: InitializeParams = parse_params(params)?;
                let r = handler.initialize()?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            LifecycleMethod::Initialized => {
                handler.initialized().map_err(internal_error)?;
                Ok(Value::Nil)
            }
        },
        Method::Process(m) => match m {
            ProcessMethod::Start => {
                let p: ExecParams = parse_params(params)?;
                let r = handler.exec(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            ProcessMethod::Read => {
                let p: ReadParams = parse_params(params)?;
                let r = handler.exec_read(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            ProcessMethod::Write => {
                let p: WriteParams = parse_params(params)?;
                let r = handler.exec_write(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            ProcessMethod::Terminate => {
                let p: TerminateParams = parse_params(params)?;
                let r = handler.terminate(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
        },
        Method::FileSystem(m) => match m {
            FsMethod::ReadFile => {
                let p: FsReadFileParams = parse_params(params)?;
                let r = handler.fs_read_file(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            FsMethod::WriteFile => {
                let p: FsWriteFileParams = parse_params(params)?;
                let r = handler.fs_write_file(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            FsMethod::CreateDirectory => {
                let p: FsCreateDirectoryParams = parse_params(params)?;
                let r = handler.fs_create_directory(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            FsMethod::GetMetadata => {
                let p: FsGetMetadataParams = parse_params(params)?;
                let r = handler.fs_get_metadata(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            FsMethod::ReadDirectory => {
                let p: FsReadDirectoryParams = parse_params(params)?;
                let r = handler.fs_read_directory(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            FsMethod::Remove => {
                let p: FsRemoveParams = parse_params(params)?;
                let r = handler.fs_remove(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            FsMethod::Copy => {
                let p: FsCopyParams = parse_params(params)?;
                let r = handler.fs_copy(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
        },
        Method::Proxy(m) => match m {
            ProxyMethod::Register => {
                let p: ProxyRegisterParams = parse_params(params)?;
                let r = handler.proxy_register(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
            ProxyMethod::Unregister => {
                let p: ProxyUnregisterParams = parse_params(params)?;
                let r = handler.proxy_unregister(p).await?;
                Ok(rmpv::ext::to_value(r).unwrap_or(Value::Nil))
            }
        },
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, JSONRPCErrorError> {
    rmpv::ext::from_value(params).map_err(|err| JSONRPCErrorError {
        code: ERROR_CODE_INVALID_PARAMS,
        message: format!("invalid params: {err}"),
        data: None,
    })
}

fn internal_error(msg: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: ERROR_CODE_INTERNAL,
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
