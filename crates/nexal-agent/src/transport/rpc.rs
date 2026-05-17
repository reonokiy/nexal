use serde::Serialize;
use tokio::sync::mpsc;

use crate::transport::protocol::JSONRPCErrorError;
use crate::transport::protocol::JSONRPCNotification;
use crate::transport::protocol::JsonRpcVersion;
use crate::transport::protocol::{ERROR_CODE_INTERNAL, ERROR_CODE_INVALID_PARAMS, ERROR_CODE_INVALID_REQUEST};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RpcServerOutboundMessage {
    Notification(JSONRPCNotification),
}

#[derive(Clone)]
pub(crate) struct RpcNotificationSender {
    outgoing_tx: mpsc::Sender<RpcServerOutboundMessage>,
}

impl RpcNotificationSender {
    pub(crate) fn new(outgoing_tx: mpsc::Sender<RpcServerOutboundMessage>) -> Self {
        Self { outgoing_tx }
    }

    pub(crate) async fn notify<P: Serialize>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<(), JSONRPCErrorError> {
        let params = rmpv::ext::to_value(params).map_err(|err| internal_error(err.to_string()))?;
        self.outgoing_tx
            .send(RpcServerOutboundMessage::Notification(
                JSONRPCNotification {
                    jsonrpc: JsonRpcVersion,
                    method: method.to_string(),
                    params: Some(params),
                },
            ))
            .await
            .map_err(|_| internal_error("RPC connection closed while sending notification".into()))
    }
}

pub(crate) fn invalid_request(message: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: ERROR_CODE_INVALID_REQUEST,
        data: None,
        message,
    }
}

pub(crate) fn invalid_params(message: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: ERROR_CODE_INVALID_PARAMS,
        data: None,
        message,
    }
}

pub(crate) fn internal_error(message: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: ERROR_CODE_INTERNAL,
        data: None,
        message,
    }
}
