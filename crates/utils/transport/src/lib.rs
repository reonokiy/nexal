//! Transport-agnostic messaging over WebSocket, stdio, and Unix sockets.
//!
//! Wraps raw I/O streams into a uniform send/receive interface with
//! background reader/writer tasks and typed message channels. Supports
//! both JSON (text frames) and MessagePack (binary frames) wire formats.

use futures::{SinkExt, StreamExt};
use rmpv::Value;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub const CHANNEL_CAPACITY: usize = 128;

#[macro_use]
mod macros;

pub mod agent;
pub mod client;
pub mod connect;
pub mod connection;
pub mod gateway;
pub mod notifications;
pub mod server;
pub mod transport;

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

#[derive(Debug)]
pub enum JsonMessageConnectionEvent<T> {
    Message(T),
    MalformedMessage { reason: String },
    Disconnected { reason: Option<String> },
}

pub struct JsonMessageConnection<T> {
    outgoing_tx: mpsc::Sender<T>,
    incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<T>>,
    task_handles: Vec<tokio::task::JoinHandle<()>>,
}

impl<T> JsonMessageConnection<T>
where
    T: DeserializeOwned + Serialize + Send + Sync + 'static,
{
    pub fn from_websocket_binary<S>(stream: WebSocketStream<S>, connection_label: String) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (incoming_tx, incoming_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (mut websocket_writer, mut websocket_reader) = stream.split();

        let reader_label = connection_label.clone();
        let incoming_tx_for_reader = incoming_tx.clone();
        let reader_task = tokio::spawn(async move {
            loop {
                match websocket_reader.next().await {
                    Some(Ok(Message::Binary(bytes))) => match rmp_serde::from_slice::<T>(&bytes) {
                        Ok(message) => {
                            if incoming_tx_for_reader
                                .send(JsonMessageConnectionEvent::Message(message))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(err) => {
                            send_malformed_message(
                                &incoming_tx_for_reader,
                                Some(format!(
                                    "failed to parse websocket msgpack from {reader_label}: {err}"
                                )),
                            )
                            .await;
                        }
                    },
                    Some(Ok(Message::Text(text))) => {
                        send_malformed_message(
                            &incoming_tx_for_reader,
                            Some(format!("unexpected text frame from {reader_label}: {text}")),
                        )
                        .await;
                    }
                    Some(Ok(Message::Close(_))) => {
                        send_disconnected(&incoming_tx_for_reader, None).await;
                        break;
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        send_disconnected(
                            &incoming_tx_for_reader,
                            Some(format!(
                                "failed to read websocket msgpack from {reader_label}: {err}"
                            )),
                        )
                        .await;
                        break;
                    }
                    None => {
                        send_disconnected(&incoming_tx_for_reader, None).await;
                        break;
                    }
                }
            }
        });

        let writer_task = tokio::spawn(async move {
            while let Some(message) = outgoing_rx.recv().await {
                match rmp_serde::to_vec(&message) {
                    Ok(encoded) => {
                        if let Err(err) =
                            websocket_writer.send(Message::Binary(encoded.into())).await
                        {
                            send_disconnected(
                                &incoming_tx,
                                Some(format!(
                                    "failed to write websocket msgpack to {connection_label}: {err}"
                                )),
                            )
                            .await;
                            break;
                        }
                    }
                    Err(err) => {
                        send_disconnected(
                            &incoming_tx,
                            Some(format!(
                                "failed to serialize msgpack for {connection_label}: {err}"
                            )),
                        )
                        .await;
                        break;
                    }
                }
            }
        });

        Self {
            outgoing_tx,
            incoming_rx,
            task_handles: vec![reader_task, writer_task],
        }
    }

    /// Create a connection over stdio / Unix socket using length-prefixed
    /// MessagePack binary frames.
    ///
    /// Frame format: 4 bytes big-endian payload length, then the
    /// MessagePack payload.
    pub fn from_stdio_binary<R, W>(reader: R, writer: W, connection_label: String) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        use tokio::io::AsyncReadExt;

        let (outgoing_tx, mut outgoing_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (incoming_tx, incoming_rx) = mpsc::channel(CHANNEL_CAPACITY);

        let reader_label = connection_label.clone();
        let incoming_tx_for_reader = incoming_tx.clone();
        let reader_task = tokio::spawn(async move {
            let mut reader = reader;
            loop {
                let mut len_buf = [0u8; 4];
                match reader.read_exact(&mut len_buf).await {
                    Ok(_) => {}
                    Err(err) => {
                        let reason = if err.kind() == std::io::ErrorKind::UnexpectedEof {
                            None
                        } else {
                            Some(format!(
                                "failed to read frame length from {reader_label}: {err}"
                            ))
                        };
                        send_disconnected(&incoming_tx_for_reader, reason).await;
                        break;
                    }
                }
                let len = u32::from_be_bytes(len_buf) as usize;
                let mut payload = vec![0u8; len];
                if let Err(err) = reader.read_exact(&mut payload).await {
                    send_disconnected(
                        &incoming_tx_for_reader,
                        Some(format!(
                            "failed to read msgpack payload from {reader_label}: {err}"
                        )),
                    )
                    .await;
                    break;
                }
                match rmp_serde::from_slice::<T>(&payload) {
                    Ok(message) => {
                        if incoming_tx_for_reader
                            .send(JsonMessageConnectionEvent::Message(message))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(err) => {
                        send_malformed_message(
                            &incoming_tx_for_reader,
                            Some(format!(
                                "failed to parse msgpack from {reader_label}: {err}"
                            )),
                        )
                        .await;
                    }
                }
            }
        });

        let writer_task = tokio::spawn(async move {
            let mut writer = writer;
            while let Some(message) = outgoing_rx.recv().await {
                let Ok(payload) = rmp_serde::to_vec(&message) else {
                    send_disconnected(
                        &incoming_tx,
                        Some(format!(
                            "failed to serialize msgpack for {connection_label}"
                        )),
                    )
                    .await;
                    break;
                };
                let len = (payload.len() as u32).to_be_bytes();
                let write_ok = writer.write_all(&len).await.is_ok()
                    && writer.write_all(&payload).await.is_ok()
                    && writer.flush().await.is_ok();
                if !write_ok {
                    send_disconnected(
                        &incoming_tx,
                        Some(format!("msgpack write to {connection_label} failed")),
                    )
                    .await;
                    break;
                }
            }
        });

        Self {
            outgoing_tx,
            incoming_rx,
            task_handles: vec![reader_task, writer_task],
        }
    }

    pub fn into_parts(
        self,
    ) -> (
        mpsc::Sender<T>,
        mpsc::Receiver<JsonMessageConnectionEvent<T>>,
        Vec<tokio::task::JoinHandle<()>>,
    ) {
        (self.outgoing_tx, self.incoming_rx, self.task_handles)
    }
}

async fn send_disconnected<T>(
    incoming_tx: &mpsc::Sender<JsonMessageConnectionEvent<T>>,
    reason: Option<String>,
) {
    let _ = incoming_tx
        .send(JsonMessageConnectionEvent::Disconnected { reason })
        .await;
}

async fn send_malformed_message<T>(
    incoming_tx: &mpsc::Sender<JsonMessageConnectionEvent<T>>,
    reason: Option<String>,
) {
    let _ = incoming_tx
        .send(JsonMessageConnectionEvent::MalformedMessage {
            reason: reason.unwrap_or_else(|| "malformed message".to_string()),
        })
        .await;
}
