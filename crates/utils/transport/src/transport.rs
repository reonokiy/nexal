//! Transport layer — binary frame delivery over WebSocket.
//!
//! Mirrors the TypeScript `packages/transport/src/transport.ts` API:
//! a [`Transport`] exposes fire-and-forget `send` plus an event
//! receiver that delivers inbound binary frames and lifecycle events.
//! Includes optional application-level heartbeat (ping/pong) to detect
//! dead connections.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use rmpv::Value;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message,
    tungstenite::client::IntoClientRequest,
};

use crate::{decode_frame, encode_frame};

pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ── Options ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HeartbeatOptions {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Default for HeartbeatOptions {
    fn default() -> Self {
        Self {
            interval: DEFAULT_HEARTBEAT_INTERVAL,
            timeout: DEFAULT_HEARTBEAT_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReconnectOptions {
    /// Max reconnect attempts. `None` = unlimited.
    pub attempts: Option<u32>,
    pub min_delay: Duration,
    pub max_delay: Duration,
    pub factor: f64,
    pub jitter: f64,
}

impl Default for ReconnectOptions {
    fn default() -> Self {
        Self {
            attempts: None,
            min_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
            factor: 2.0,
            jitter: 0.2,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TransportOptions {
    pub connect_timeout: Option<Duration>,
    pub heartbeat: Option<HeartbeatOptions>,
    pub reconnect: Option<ReconnectOptions>,
}

// ── Events ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum TransportEvent {
    /// Inbound application-level binary frame.
    Frame(Vec<u8>),
    /// Heartbeat pong did not arrive within the timeout window.
    HeartbeatDead,
    /// Underlying socket disconnected (after exhausting any reconnect attempts).
    Disconnected,
    /// Client reconnected successfully after a prior disconnect.
    Reconnected,
}

// ── Driver commands ─────────────────────────────────────────────────

enum DriverCmd {
    Send(Vec<u8>),
    StartHeartbeat,
    StopHeartbeat,
    Close,
}

// ── Transport handle ────────────────────────────────────────────────

#[derive(Clone)]
pub struct Transport {
    cmd_tx: mpsc::UnboundedSender<DriverCmd>,
    closed: Arc<AtomicBool>,
}

impl Transport {
    pub fn send(&self, frame: Vec<u8>) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.cmd_tx.send(DriverCmd::Send(frame));
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.cmd_tx.send(DriverCmd::Close);
    }

    pub fn start_heartbeat(&self) {
        let _ = self.cmd_tx.send(DriverCmd::StartHeartbeat);
    }

    pub fn stop_heartbeat(&self) {
        let _ = self.cmd_tx.send(DriverCmd::StopHeartbeat);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

// ── Ping/pong frames ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct PingPongFrame {
    method: String,
}

fn ping_frame_bytes() -> Vec<u8> {
    encode_frame(&PingPongFrame {
        method: "ping".into(),
    })
    .expect("encode ping")
}

fn pong_frame_bytes() -> Vec<u8> {
    encode_frame(&PingPongFrame {
        method: "pong".into(),
    })
    .expect("encode pong")
}

/// Returns `Some(true)` if `bytes` decodes to a heartbeat ping frame,
/// `Some(false)` for a heartbeat pong frame, `None` otherwise.
///
/// A heartbeat frame is exactly `{method: "ping"}` or `{method: "pong"}`
/// with no other keys (no `id`, no `params`, no `stream`). Application
/// notifications happen to share the "ping"/"pong" name space, so the
/// stricter shape check avoids mis-classifying them.
fn classify_ping_pong(bytes: &[u8]) -> Option<bool> {
    let value: Value = decode_frame(bytes).ok()?;
    let map = value.as_map()?;
    if map.len() != 1 {
        return None;
    }
    let (k, v) = map.first()?;
    if k.as_str() != Some("method") {
        return None;
    }
    match v.as_str()? {
        "ping" => Some(true),
        "pong" => Some(false),
        _ => None,
    }
}

// ── Accepted (server-side) WebSocket transport ──────────────────────

/// Wrap a server-accepted [`WebSocketStream`] into a [`Transport`].
///
/// Spawns a background driver task that pumps the socket, intercepts
/// ping/pong frames, and emits [`TransportEvent`]s on the returned
/// receiver.
pub fn accepted_websocket_transport<S>(
    stream: WebSocketStream<S>,
    options: TransportOptions,
) -> (Transport, mpsc::UnboundedReceiver<TransportEvent>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();
    let closed = Arc::new(AtomicBool::new(false));

    let closed_for_driver = closed.clone();
    tokio::spawn(async move {
        run_accepted_driver(stream, cmd_rx, evt_tx, options, closed_for_driver).await;
    });

    (Transport { cmd_tx, closed }, evt_rx)
}

async fn run_accepted_driver<S>(
    stream: WebSocketStream<S>,
    mut cmd_rx: mpsc::UnboundedReceiver<DriverCmd>,
    evt_tx: mpsc::UnboundedSender<TransportEvent>,
    options: TransportOptions,
    closed: Arc<AtomicBool>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut ws_writer, mut ws_reader) = stream.split();

    let hb_opts = options.heartbeat.clone().unwrap_or_default();
    let mut hb_enabled = options.heartbeat.is_some();
    let mut hb_interval = make_heartbeat_interval(hb_enabled, &hb_opts);
    let mut hb_deadline: Option<Instant> = None;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(DriverCmd::Send(bytes)) => {
                    if ws_writer.send(Message::Binary(bytes.into())).await.is_err() {
                        let _ = evt_tx.send(TransportEvent::Disconnected);
                        break;
                    }
                }
                Some(DriverCmd::StartHeartbeat) => {
                    hb_enabled = true;
                    hb_interval = make_heartbeat_interval(true, &hb_opts);
                }
                Some(DriverCmd::StopHeartbeat) => {
                    hb_enabled = false;
                    hb_interval = None;
                    hb_deadline = None;
                }
                Some(DriverCmd::Close) | None => {
                    let _ = ws_writer.close().await;
                    break;
                }
            },
            _ = tick_or_pending(hb_interval.as_mut()), if hb_enabled => {
                if ws_writer.send(Message::Binary(ping_frame_bytes().into())).await.is_err() {
                    let _ = evt_tx.send(TransportEvent::Disconnected);
                    break;
                }
                hb_deadline = Some(Instant::now() + hb_opts.timeout);
            }
            _ = sleep_until_or_pending(hb_deadline), if hb_deadline.is_some() => {
                hb_deadline = None;
                let _ = evt_tx.send(TransportEvent::HeartbeatDead);
            }
            msg = ws_reader.next() => {
                match msg {
                    Some(Ok(Message::Binary(bytes))) => {
                        let bytes_vec = bytes.to_vec();
                        match classify_ping_pong(&bytes_vec) {
                            Some(true) => {
                                if ws_writer.send(Message::Binary(pong_frame_bytes().into())).await.is_err() {
                                    let _ = evt_tx.send(TransportEvent::Disconnected);
                                    break;
                                }
                            }
                            Some(false) => { hb_deadline = None; }
                            None => {
                                if evt_tx.send(TransportEvent::Frame(bytes_vec)).is_err() { break; }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        let _ = evt_tx.send(TransportEvent::Disconnected);
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => {
                        let _ = evt_tx.send(TransportEvent::Disconnected);
                        break;
                    }
                }
            }
        }
    }

    closed.store(true, Ordering::SeqCst);
}

// ── Client (dial) WebSocket transport ───────────────────────────────

/// Errors from establishing a client WebSocket transport.
#[derive(Debug, thiserror::Error)]
pub enum ClientTransportError {
    #[error("connect timeout to {0}")]
    Timeout(String),
    #[error("connect error to {url}: {source}")]
    Connect {
        url: String,
        #[source]
        source: tokio_tungstenite::tungstenite::Error,
    },
}

/// Connect to `url` and return a [`Transport`] + event receiver.
///
/// Performs the initial connect inline; reconnect logic (if configured)
/// runs in the background driver task.
pub async fn connect_websocket_transport(
    url: impl Into<String>,
    options: TransportOptions,
) -> Result<(Transport, mpsc::UnboundedReceiver<TransportEvent>), ClientTransportError> {
    let url: String = url.into();
    let stream = dial_once(&url, options.connect_timeout).await?;

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();
    let closed = Arc::new(AtomicBool::new(false));

    let closed_for_driver = closed.clone();
    let url_for_driver = url.clone();
    tokio::spawn(async move {
        run_client_driver(
            url_for_driver,
            stream,
            cmd_rx,
            evt_tx,
            options,
            closed_for_driver,
        )
        .await;
    });

    Ok((Transport { cmd_tx, closed }, evt_rx))
}

type ClientWs = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn dial_once(
    url: &str,
    connect_timeout: Option<Duration>,
) -> Result<ClientWs, ClientTransportError> {
    let request = url
        .into_client_request()
        .map_err(|e| ClientTransportError::Connect {
            url: url.into(),
            source: e,
        })?;
    let timeout = connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
    let fut = connect_async(request);
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok((stream, _))) => Ok(stream),
        Ok(Err(e)) => Err(ClientTransportError::Connect {
            url: url.into(),
            source: e,
        }),
        Err(_) => Err(ClientTransportError::Timeout(url.into())),
    }
}

async fn run_client_driver(
    url: String,
    initial: ClientWs,
    mut cmd_rx: mpsc::UnboundedReceiver<DriverCmd>,
    evt_tx: mpsc::UnboundedSender<TransportEvent>,
    options: TransportOptions,
    closed: Arc<AtomicBool>,
) {
    let mut stream_opt: Option<ClientWs> = Some(initial);
    let mut reconnect_attempts: u32 = 0;

    let hb_opts = options.heartbeat.clone().unwrap_or_default();
    let mut hb_enabled = options.heartbeat.is_some();

    'outer: loop {
        let Some(stream) = stream_opt.take() else {
            // Try to reconnect or bail.
            match attempt_reconnect(
                &url,
                &options,
                &mut reconnect_attempts,
                &mut cmd_rx,
                &closed,
            )
            .await
            {
                ReconnectOutcome::Connected(s) => {
                    let _ = evt_tx.send(TransportEvent::Reconnected);
                    stream_opt = Some(s);
                    continue;
                }
                ReconnectOutcome::Closed => break 'outer,
                ReconnectOutcome::GaveUp => {
                    let _ = evt_tx.send(TransportEvent::Disconnected);
                    break 'outer;
                }
            }
        };

        let (mut ws_writer, mut ws_reader) = stream.split();
        let mut hb_interval = make_heartbeat_interval(hb_enabled, &hb_opts);
        let mut hb_deadline: Option<Instant> = None;

        // Inner loop returns once the socket disconnects so the outer loop
        // can attempt to reconnect (if enabled).
        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => match cmd {
                    Some(DriverCmd::Send(bytes)) => {
                        if ws_writer.send(Message::Binary(bytes.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(DriverCmd::StartHeartbeat) => {
                        hb_enabled = true;
                        hb_interval = make_heartbeat_interval(true, &hb_opts);
                    }
                    Some(DriverCmd::StopHeartbeat) => {
                        hb_enabled = false;
                        hb_interval = None;
                        hb_deadline = None;
                    }
                    Some(DriverCmd::Close) | None => {
                        let _ = ws_writer.close().await;
                        closed.store(true, Ordering::SeqCst);
                        return;
                    }
                },
                _ = tick_or_pending(hb_interval.as_mut()), if hb_enabled => {
                    if ws_writer.send(Message::Binary(ping_frame_bytes().into())).await.is_err() {
                        break;
                    }
                    hb_deadline = Some(Instant::now() + hb_opts.timeout);
                }
                _ = sleep_until_or_pending(hb_deadline), if hb_deadline.is_some() => {
                    hb_deadline = None;
                    let _ = evt_tx.send(TransportEvent::HeartbeatDead);
                }
                msg = ws_reader.next() => {
                    match msg {
                        Some(Ok(Message::Binary(bytes))) => {
                            let bytes_vec = bytes.to_vec();
                            match classify_ping_pong(&bytes_vec) {
                                Some(true) => {
                                    if ws_writer.send(Message::Binary(pong_frame_bytes().into())).await.is_err() {
                                        break;
                                    }
                                }
                                Some(false) => { hb_deadline = None; }
                                None => {
                                    if evt_tx.send(TransportEvent::Frame(bytes_vec)).is_err() {
                                        closed.store(true, Ordering::SeqCst);
                                        return;
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(_)) => break,
                    }
                }
            }
        }

        // Socket disconnected. Drop both halves and either bail or reconnect.
        drop(ws_writer);
        drop(ws_reader);
        if options.reconnect.is_none() {
            let _ = evt_tx.send(TransportEvent::Disconnected);
            closed.store(true, Ordering::SeqCst);
            return;
        }
        // stream_opt stays None -> reconnect branch above.
    }
}

enum ReconnectOutcome {
    Connected(ClientWs),
    Closed,
    GaveUp,
}

async fn attempt_reconnect(
    url: &str,
    options: &TransportOptions,
    attempts: &mut u32,
    cmd_rx: &mut mpsc::UnboundedReceiver<DriverCmd>,
    closed: &Arc<AtomicBool>,
) -> ReconnectOutcome {
    let Some(rc) = options.reconnect.clone() else {
        return ReconnectOutcome::GaveUp;
    };

    loop {
        if let Some(max) = rc.attempts {
            if *attempts >= max {
                return ReconnectOutcome::GaveUp;
            }
        }
        *attempts += 1;
        let delay = reconnect_delay(&rc, *attempts);

        // Wait the delay, but allow Close to interrupt.
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(DriverCmd::Close) | None => {
                        closed.store(true, Ordering::SeqCst);
                        return ReconnectOutcome::Closed;
                    }
                    // Drop other commands during the wait.
                    _ => {}
                }
            }
        }

        match dial_once(url, options.connect_timeout).await {
            Ok(stream) => {
                *attempts = 0;
                return ReconnectOutcome::Connected(stream);
            }
            Err(_) => {
                continue;
            }
        }
    }
}

fn reconnect_delay(rc: &ReconnectOptions, attempt: u32) -> Duration {
    let attempt = attempt.max(1) - 1;
    let base_ms = (rc.min_delay.as_millis() as f64) * rc.factor.powi(attempt as i32);
    let capped = base_ms.min(rc.max_delay.as_millis() as f64);
    let jitter = capped * rc.jitter * pseudo_random_unit();
    Duration::from_millis((capped + jitter).round() as u64)
}

fn pseudo_random_unit() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos as f64) / 1_000_000_000.0
}

// ── Helpers ─────────────────────────────────────────────────────────

fn make_heartbeat_interval(
    enabled: bool,
    opts: &HeartbeatOptions,
) -> Option<tokio::time::Interval> {
    if !enabled {
        return None;
    }
    let mut i = tokio::time::interval_at(Instant::now() + opts.interval, opts.interval);
    i.set_missed_tick_behavior(MissedTickBehavior::Skip);
    Some(i)
}

async fn tick_or_pending(interval: Option<&mut tokio::time::Interval>) -> Instant {
    match interval {
        Some(i) => i.tick().await,
        None => std::future::pending().await,
    }
}

async fn sleep_until_or_pending(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}
