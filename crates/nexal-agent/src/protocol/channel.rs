//! Message helpers for the bidirectional WS + msgpack channel.

use rmpv::Value;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::protocol::errors::{ChannelError, ChannelErrorKind};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChannelOutboundMessage {
    Notification(Value),
}

#[derive(Clone)]
pub(crate) struct ChannelNotificationSender {
    outgoing_tx: mpsc::Sender<ChannelOutboundMessage>,
}

impl ChannelNotificationSender {
    pub(crate) fn new(outgoing_tx: mpsc::Sender<ChannelOutboundMessage>) -> Self {
        Self { outgoing_tx }
    }

    pub(crate) async fn notify<P: Serialize>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<(), ChannelError> {
        let params = rmpv::ext::to_value(params).map_err(|err| internal_error(err.to_string()))?;
        let msg = Value::Map(vec![
            (Value::String("method".into()), Value::String(method.into())),
            (Value::String("params".into()), params),
        ]);
        self.outgoing_tx
            .send(ChannelOutboundMessage::Notification(msg))
            .await
            .map_err(|_| internal_error("channel closed while sending notification".into()))
    }
}

pub(crate) fn invalid_request(message: String) -> ChannelError {
    ChannelError {
        kind: ChannelErrorKind::InvalidRequest,
        data: None,
        message,
    }
}

pub(crate) fn invalid_params(message: String) -> ChannelError {
    ChannelError {
        kind: ChannelErrorKind::InvalidParams,
        data: None,
        message,
    }
}

pub(crate) fn internal_error(message: String) -> ChannelError {
    ChannelError {
        kind: ChannelErrorKind::Internal,
        data: None,
        message,
    }
}
