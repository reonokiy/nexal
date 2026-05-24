//! Transport-agnostic messaging over WebSocket, stdio, and Unix sockets.
//!
//! Layout (mirrors the TS `@nexal/transport` package):
//!   - [`mod@transport`]                — WS bytes, heartbeat, reconnect
//!   - [`mod@connection`]               — [`Connection`] / [`Stream`] + WS connect helpers
//!   - [`message_channel`]              — legacy msgpack WS/stdio channel
//!     ([`MessageChannel`]) still used where [`Connection`] hasn't landed yet
//!   - [`mod@agent`], [`mod@gateway`]   — per-protocol method matrix +
//!     typed client/server factories ([`AgentClient`], [`GatewayClient`],
//!     [`GatewayAgentClient`], [`AgentHandlers`], [`GatewayHandlers`],
//!     [`serve_agent`], [`serve_gateway`])
//!
//! Wire envelope, codec helpers, and the [`RpcMethod`] trait live at
//! the crate root.

use rmpv::Value;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const CHANNEL_CAPACITY: usize = 128;

#[macro_use]
mod macros;

pub mod agent;
pub mod connection;
pub mod gateway;
pub mod message_channel;
pub mod transport;

// ── Convenient top-level re-exports ─────────────────────────────────

pub use agent::client::AgentClient;
pub use agent::server::{AgentHandlers, serve_agent};
pub use gateway::client::{GatewayAgentClient, GatewayClient};
pub use gateway::server::{GatewayHandlers, serve_gateway};
pub use message_channel::{MessageChannel, MessageChannelEvent};

// ── Wire Envelope ────────────────────────────────────────────────────

pub type MessageId = Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    pub id: MessageId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    pub id: MessageId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireNotification {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WireMessage {
    Request(WireRequest),
    Response(WireResponse),
    Notification(WireNotification),
}

impl WireResponse {
    pub fn ok(id: MessageId, result: Value) -> Self {
        Self {
            stream: None,
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: MessageId, code: i32, message: impl Into<String>) -> Self {
        Self {
            stream: None,
            id,
            result: None,
            error: Some(WireError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

pub fn notification(method: impl Into<String>, params: Option<Value>) -> WireNotification {
    WireNotification {
        stream: None,
        method: method.into(),
        params,
    }
}

/// Construct a JSON-RPC style "method not found" [`WireError`].
pub fn method_not_found(method: &str) -> WireError {
    WireError {
        code: -32601,
        message: format!("method not found: {method}"),
        data: None,
    }
}

// ── Codec helpers ────────────────────────────────────────────────────

pub fn encode_frame<T: Serialize>(frame: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut buf = Vec::new();
    let mut ser = rmp_serde::Serializer::new(&mut buf).with_struct_map();
    frame.serialize(&mut ser)?;
    Ok(buf)
}

pub fn to_msgpack_value<T: Serialize>(v: &T) -> Result<Value, String> {
    let bytes = encode_frame(v).map_err(|err| err.to_string())?;
    rmp_serde::from_slice(&bytes).map_err(|err| err.to_string())
}

/// Decode an [`rmpv::Value`] into a serde-deserializable `T`.
///
/// Implemented by round-tripping through msgpack bytes (rather than
/// [`rmpv::ext::from_value`]), so that serde-specific encodings such as
/// `#[serde(rename_all)]` enums map back to their string discriminants
/// correctly — matching what [`to_msgpack_value`] produces.
pub fn from_msgpack_value<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    let mut buf = Vec::new();
    rmpv::encode::write_value(&mut buf, &value).map_err(|err| err.to_string())?;
    rmp_serde::from_slice(&buf).map_err(|err| err.to_string())
}

pub fn decode_frame<T: DeserializeOwned>(data: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    rmp_serde::from_slice(data)
}

pub fn decode_wire_message(data: &[u8]) -> Result<WireMessage, rmp_serde::decode::Error> {
    let value: Value = decode_frame(data)?;
    Ok(value_to_wire_message(value))
}

pub fn value_to_wire_message(value: Value) -> WireMessage {
    let has_id = map_get(&value, "id").is_some();
    let has_method = map_get(&value, "method").is_some();
    let has_result = map_get(&value, "result").is_some();
    let has_error = map_get(&value, "error").is_some();

    if has_id && has_method {
        WireMessage::Request(rmpv::ext::from_value(value).expect("wire request shape"))
    } else if has_id && (has_result || has_error) {
        WireMessage::Response(rmpv::ext::from_value(value).expect("wire response shape"))
    } else {
        WireMessage::Notification(rmpv::ext::from_value(value).expect("wire notification shape"))
    }
}

fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_map()?.iter().find_map(|(k, v)| {
        if k.as_str() == Some(key) {
            Some(v)
        } else {
            None
        }
    })
}

// ── Typed RPC method matrix ─────────────────────────────────────────

pub trait RpcMethod {
    const METHOD: &'static str;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

/// Ergonomic call-site helpers for any [`RpcMethod`]: lets callers write
///
/// ```ignore
/// AgentInitialize::call(&conn, &params).await?
/// ```
///
/// without first wrapping the connection in a typed client.
pub trait RpcMethodExt: RpcMethod + Sized {
    fn call<'a>(
        conn: &'a connection::Connection,
        params: &'a Self::Params,
    ) -> impl std::future::Future<Output = Result<Self::Result, connection::TypedRequestError>>
    + Send
    + 'a
    where
        Self::Params: Sync,
    {
        async move { conn.request_typed::<Self>(params).await }
    }

    fn call_on_stream<'a>(
        stream: &'a connection::Stream,
        params: &'a Self::Params,
    ) -> impl std::future::Future<Output = Result<Self::Result, connection::TypedRequestError>>
    + Send
    + 'a
    where
        Self::Params: Sync,
    {
        async move { stream.request_typed::<Self>(params).await }
    }
}

impl<T: RpcMethod + Sized> RpcMethodExt for T {}

pub fn typed_request<M: RpcMethod>(
    id: MessageId,
    params: &M::Params,
) -> Result<WireRequest, String> {
    let params = to_msgpack_value(params)?;
    Ok(WireRequest {
        stream: None,
        id,
        method: M::METHOD.into(),
        params: Some(params),
    })
}

pub fn typed_notification<T: Serialize>(
    method: impl Into<String>,
    params: &T,
) -> Result<WireNotification, String> {
    let params = to_msgpack_value(params)?;
    Ok(WireNotification {
        stream: None,
        method: method.into(),
        params: Some(params),
    })
}
