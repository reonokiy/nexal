use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
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
    pub fn from_stdio<R, W>(reader: R, writer: W, connection_label: String) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (incoming_tx, incoming_rx) = mpsc::channel(CHANNEL_CAPACITY);

        let reader_label = connection_label.clone();
        let incoming_tx_for_reader = incoming_tx.clone();
        let reader_task = tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<T>(&line) {
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
                                        "failed to parse JSON message from {reader_label}: {err}"
                                    )),
                                )
                                .await;
                            }
                        }
                    }
                    Ok(None) => {
                        send_disconnected(&incoming_tx_for_reader, None).await;
                        break;
                    }
                    Err(err) => {
                        send_disconnected(
                            &incoming_tx_for_reader,
                            Some(format!(
                                "failed to read JSON message from {reader_label}: {err}"
                            )),
                        )
                        .await;
                        break;
                    }
                }
            }
        });

        let writer_task = tokio::spawn(async move {
            let mut writer = BufWriter::new(writer);
            while let Some(message) = outgoing_rx.recv().await {
                if let Err(err) = write_line_message(&mut writer, &message).await {
                    send_disconnected(
                        &incoming_tx,
                        Some(format!(
                            "failed to write JSON message to {connection_label}: {err}"
                        )),
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

    pub fn from_websocket<S>(stream: WebSocketStream<S>, connection_label: String) -> Self
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
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<T>(text.as_ref()) {
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
                                        "failed to parse websocket JSON message from {reader_label}: {err}"
                                    )),
                                )
                                .await;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        match serde_json::from_slice::<T>(bytes.as_ref()) {
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
                                        "failed to parse websocket JSON message from {reader_label}: {err}"
                                    )),
                                )
                                .await;
                            }
                        }
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
                                "failed to read websocket JSON message from {reader_label}: {err}"
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
                match serde_json::to_string(&message) {
                    Ok(encoded) => {
                        if let Err(err) = websocket_writer.send(Message::Text(encoded.into())).await
                        {
                            send_disconnected(
                                &incoming_tx,
                                Some(format!(
                                    "failed to write websocket JSON message to {connection_label}: {err}"
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
                                "failed to serialize JSON message for {connection_label}: {err}"
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
            reason: reason.unwrap_or_else(|| "malformed JSON message".to_string()),
        })
        .await;
}

async fn write_line_message<W, T>(writer: &mut BufWriter<W>, message: &T) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let encoded =
        serde_json::to_string(message).map_err(|err| std::io::Error::other(err.to_string()))?;
    writer.write_all(encoded.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}
