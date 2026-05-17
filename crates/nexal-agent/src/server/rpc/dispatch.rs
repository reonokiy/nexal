//! WebTransport JSON-RPC dispatch.
//!
//! Replaces the jsonrpsee-based transport with manual method dispatch
//! over a newline-delimited JSON stream (typically a WebTransport
//! bidirectional stream wrapped by `JsonMessageConnection`).
//!
//! The `ExecServerHandler` (business logic) is unchanged — this module
//! only handles serialization, method routing, and notification
//! forwarding.

use std::sync::Arc;

use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::protocol::{
    ExecParams, FsCopyParams, FsCreateDirectoryParams, FsGetMetadataParams, FsReadDirectoryParams,
    FsReadFileParams, FsRemoveParams, FsWriteFileParams, InitializeParams, JSONRPCErrorError,
    ProxyRegisterParams, ProxyUnregisterParams, ReadParams, TerminateParams, WriteParams,
    EXEC_CLOSED_METHOD, EXEC_EXITED_METHOD, EXEC_OUTPUT_DELTA_METHOD,
};
use crate::server::services::{ExecServerHandler, ProcessEvent};

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
    // Forward process events as JSON-RPC notifications.
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
                            serde_json::to_value(n).unwrap_or(Value::Null),
                        ),
                        ProcessEvent::Exited(n) => (
                            EXEC_EXITED_METHOD,
                            serde_json::to_value(n).unwrap_or(Value::Null),
                        ),
                        ProcessEvent::Closed(n) => (
                            EXEC_CLOSED_METHOD,
                            serde_json::to_value(n).unwrap_or(Value::Null),
                        ),
                    };
                    let notif = json!({
                        "jsonrpc": "2.0",
                        "method": method,
                        "params": params,
                    });
                    if notify_tx.send(notif).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Request dispatch loop.
    while let Some(event) = incoming_rx.recv().await {
        match event {
            JsonMessageConnectionEvent::Message(value) => {
                let id = value.get("id").cloned();
                let method = value
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let params = value.get("params").cloned().unwrap_or(Value::Null);

                // Unwrap positional params — the gateway wraps single
                // objects in an array for jsonrpsee compat. Accept both.
                let params = unwrap_positional(params);

                let result = dispatch(&handler, &method, params).await;

                // Only send a response if the request has an id (not a notification).
                if let Some(req_id) = id {
                    let response = match result {
                        Ok(val) => json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": val,
                        }),
                        Err(err) => json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "error": {
                                "code": err.code,
                                "message": err.message,
                                "data": err.data,
                            },
                        }),
                    };
                    if outgoing_tx.send(response).await.is_err() {
                        break;
                    }
                }
            }
            JsonMessageConnectionEvent::MalformedMessage { reason } => {
                warn!("malformed message: {reason}");
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": "Parse error",
                        "data": reason,
                    },
                });
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

/// Build a dispatch loop from a `JsonMessageConnection`.
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
    match method {
        "initialize" => {
            let _p: InitializeParams = parse_params(params)?;
            let r = handler.initialize()?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "initialized" => {
            handler.initialized().map_err(internal_error)?;
            Ok(Value::Null)
        }
        "process/start" => {
            tracing::debug!("dispatch: process/start parsing params");
            let p: ExecParams = parse_params(params)?;
            tracing::debug!("dispatch: process/start calling handler.exec");
            let r = handler.exec(p).await?;
            tracing::debug!("dispatch: process/start exec returned {:?}", r);
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "process/read" => {
            let p: ReadParams = parse_params(params)?;
            let r = handler.exec_read(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "process/write" => {
            let p: WriteParams = parse_params(params)?;
            let r = handler.exec_write(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "process/terminate" => {
            let p: TerminateParams = parse_params(params)?;
            let r = handler.terminate(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/read_file" => {
            let p: FsReadFileParams = parse_params(params)?;
            let r = handler.fs_read_file(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/write_file" => {
            let p: FsWriteFileParams = parse_params(params)?;
            let r = handler.fs_write_file(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/create_directory" => {
            let p: FsCreateDirectoryParams = parse_params(params)?;
            let r = handler.fs_create_directory(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/get_metadata" => {
            let p: FsGetMetadataParams = parse_params(params)?;
            let r = handler.fs_get_metadata(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/read_directory" => {
            let p: FsReadDirectoryParams = parse_params(params)?;
            let r = handler.fs_read_directory(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/remove" => {
            let p: FsRemoveParams = parse_params(params)?;
            let r = handler.fs_remove(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "fs/copy" => {
            let p: FsCopyParams = parse_params(params)?;
            let r = handler.fs_copy(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "proxy/register" => {
            let p: ProxyRegisterParams = parse_params(params)?;
            let r = handler.proxy_register(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        "proxy/unregister" => {
            let p: ProxyUnregisterParams = parse_params(params)?;
            let r = handler.proxy_unregister(p).await?;
            Ok(serde_json::to_value(r).unwrap_or(Value::Null))
        }
        other => Err(JSONRPCErrorError {
            code: -32601,
            message: format!("method not found: {other}"),
            data: None,
        }),
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, JSONRPCErrorError> {
    serde_json::from_value(params).map_err(|err| JSONRPCErrorError {
        code: -32602,
        message: format!("invalid params: {err}"),
        data: None,
    })
}

fn internal_error(msg: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32603,
        message: msg,
        data: None,
    }
}

/// Unwrap jsonrpsee-style positional params.
/// `[{...}]` → `{...}`, `[]` → `null`, anything else → passthrough.
fn unwrap_positional(params: Value) -> Value {
    match params {
        Value::Array(mut arr) if arr.len() == 1 => arr.remove(0),
        Value::Array(arr) if arr.is_empty() => Value::Null,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwrap_single_element_array() {
        let input = json!([{"name": "test"}]);
        assert_eq!(unwrap_positional(input), json!({"name": "test"}));
    }

    #[test]
    fn unwrap_empty_array() {
        assert_eq!(unwrap_positional(json!([])), Value::Null);
    }

    #[test]
    fn passthrough_object() {
        let input = json!({"name": "test"});
        assert_eq!(unwrap_positional(input.clone()), input);
    }

    #[test]
    fn passthrough_null() {
        assert_eq!(unwrap_positional(Value::Null), Value::Null);
    }
}
