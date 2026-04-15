use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};

use crate::protocol::JSONRPCMessage;

pub(crate) use nexal_utils_json_transport::CHANNEL_CAPACITY;

pub(crate) type JsonRpcConnection = JsonMessageConnection<JSONRPCMessage>;
pub(crate) type JsonRpcConnectionEvent = JsonMessageConnectionEvent<JSONRPCMessage>;
