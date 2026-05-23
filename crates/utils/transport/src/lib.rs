//! Transport-agnostic messaging over WebSocket, stdio, and Unix sockets.
//!
//! Wraps raw I/O streams into a uniform send/receive interface with
//! background reader/writer tasks and typed message channels. Supports
//! both JSON (text frames) and MessagePack (binary frames) wire formats.

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub const CHANNEL_CAPACITY: usize = 128;

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
