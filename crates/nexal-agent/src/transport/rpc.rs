use crate::protocol::JSONRPCErrorError;
use crate::protocol::JSONRPCNotification;
use serde::Serialize;
use tokio::sync::mpsc;

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
        let params = serde_json::to_value(params).map_err(|err| internal_error(err.to_string()))?;
        self.outgoing_tx
            .send(RpcServerOutboundMessage::Notification(
                JSONRPCNotification {
                    jsonrpc: jsonrpsee::types::TwoPointZero,
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
        code: -32600,
        data: None,
        message,
    }
}

pub(crate) fn invalid_params(message: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32602,
        data: None,
        message,
    }
}

pub(crate) fn internal_error(message: String) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code: -32603,
        data: None,
        message,
    }
}
