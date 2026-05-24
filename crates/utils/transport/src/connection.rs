//! Connection — high-level multiplexed message layer over a Transport.
//!
//! Mirrors the TypeScript `packages/transport/src/connection.ts`:
//! supports virtual streams (multiple logical channels over one
//! transport), per-stream request/response correlation, and
//! notification/request handlers.
//!
//! # Example
//!
//! ```no_run
//! # use nexal_utils_transport::connection::Connection;
//! # use nexal_utils_transport::transport::{accepted_websocket_transport, TransportOptions};
//! # async fn demo<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static>(
//! #   ws: tokio_tungstenite::WebSocketStream<S>,
//! # ) -> Result<(), Box<dyn std::error::Error>> {
//! let (transport, events) = accepted_websocket_transport(ws, TransportOptions::default());
//! let conn = Connection::new(transport, events);
//!
//! // connection-level request
//! let hello = conn.request("gateway/hello", None).await?;
//!
//! // virtual stream for one agent
//! let agent = conn.stream("agent-1");
//! let result = agent.request("process/start", None).await?;
//! agent.notify("ping", None);
//! agent.close();
//! # Ok(()) }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Weak;

use futures::future::BoxFuture;
use rmpv::Value;
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::transport::{Transport, TransportEvent};
use crate::{
    MessageId, RpcMethod, WireError, WireMessage, WireNotification, WireRequest, WireResponse,
    encode_frame, from_msgpack_value, to_msgpack_value, value_to_wire_message,
};

/// Handler invoked for inbound notifications.
pub type NotificationHandler = Arc<dyn Fn(Value) + Send + Sync>;

/// Handler invoked for inbound requests; returns a future producing a
/// result or a [`WireError`].
pub type RequestHandler =
    Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value, WireError>> + Send + Sync>;

/// Errors that can be produced by [`Connection::request`] and
/// [`Stream::request`].
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("connection closed")]
    Closed,
    #[error("{0:?}")]
    Wire(WireError),
}

/// Errors that can be produced by the typed `*_typed` request helpers
/// (encode/decode plus underlying [`RequestError`]).
#[derive(Debug, thiserror::Error)]
pub enum TypedRequestError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("encode params: {0}")]
    EncodeParams(String),
    #[error("decode result: {0}")]
    DecodeResult(String),
}

// ── Per-channel state ───────────────────────────────────────────────

struct ChannelState {
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, WireError>>>>,
    notification_handlers: Mutex<HashMap<String, Vec<NotificationHandler>>>,
    request_handlers: Mutex<HashMap<String, RequestHandler>>,
}

impl ChannelState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            notification_handlers: Mutex::new(HashMap::new()),
            request_handlers: Mutex::new(HashMap::new()),
        })
    }

    async fn reject_all_pending(&self, reason: &str) {
        let mut pending = self.pending.lock().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(WireError {
                code: -32000,
                message: reason.into(),
                data: None,
            }));
        }
    }

    async fn dispatch(self: &Arc<Self>, msg: WireMessage, sender: SendFn) {
        match msg {
            WireMessage::Response(resp) => {
                let key = id_to_key(&resp.id);
                let slot = {
                    let mut pending = self.pending.lock().await;
                    pending.remove(&key)
                };
                if let Some(tx) = slot {
                    let result = match resp.error {
                        Some(err) => Err(err),
                        None => Ok(resp.result.unwrap_or(Value::Nil)),
                    };
                    let _ = tx.send(result);
                }
            }
            WireMessage::Notification(notif) => {
                let handlers = {
                    let map = self.notification_handlers.lock().await;
                    map.get(&notif.method).cloned()
                };
                if let Some(handlers) = handlers {
                    let params = notif.params.unwrap_or(Value::Nil);
                    for h in handlers {
                        h(params.clone());
                    }
                }
            }
            WireMessage::Request(req) => {
                let handler = {
                    let map = self.request_handlers.lock().await;
                    map.get(&req.method).cloned()
                };
                let Some(handler) = handler else {
                    return;
                };
                let stream = req.stream.clone();
                let id = req.id.clone();
                let params = req.params.unwrap_or(Value::Nil);
                tokio::spawn(async move {
                    let result = handler(params).await;
                    let resp = match result {
                        Ok(value) => WireResponse {
                            stream,
                            id,
                            result: Some(value),
                            error: None,
                        },
                        Err(error) => WireResponse {
                            stream,
                            id,
                            result: None,
                            error: Some(error),
                        },
                    };
                    sender.send_response(resp);
                });
            }
        }
    }
}

fn id_to_key(id: &MessageId) -> String {
    match id {
        Value::String(s) => s.as_str().map(str::to_string).unwrap_or_else(|| id.to_string()),
        Value::Integer(i) => i.to_string(),
        other => other.to_string(),
    }
}

// ── Send helper passed into spawned request handler tasks ───────────

#[derive(Clone)]
struct SendFn {
    transport: Transport,
}

impl SendFn {
    fn send_response(&self, resp: WireResponse) {
        if let Ok(bytes) = encode_frame(&resp) {
            self.transport.send(bytes);
        }
    }
}

// ── Connection ──────────────────────────────────────────────────────

struct ConnectionInner {
    transport: Transport,
    root: Arc<ChannelState>,
    /// Streams map uses a sync mutex (very short critical sections) so
    /// [`Connection::stream`] can be called from sync and async contexts
    /// without risk of panicking inside a tokio runtime.
    streams: std::sync::Mutex<HashMap<String, Arc<ChannelState>>>,
}

/// Multiplexed RPC connection over a [`Transport`].
#[derive(Clone)]
pub struct Connection(Arc<ConnectionInner>);

impl Connection {
    /// Wrap a transport and event stream into a Connection.
    ///
    /// Spawns a background task that pumps inbound frames and dispatches
    /// them to pending requests, notification handlers, and request
    /// handlers (per connection-level or per stream).
    pub fn new(
        transport: Transport,
        mut events: mpsc::UnboundedReceiver<TransportEvent>,
    ) -> Self {
        let inner = Arc::new(ConnectionInner {
            transport: transport.clone(),
            root: ChannelState::new(),
            streams: std::sync::Mutex::new(HashMap::new()),
        });

        let weak: Weak<ConnectionInner> = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Some(ev) = events.recv().await {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                match ev {
                    TransportEvent::Frame(bytes) => {
                        inner.handle_frame(bytes).await;
                    }
                    TransportEvent::Disconnected => {
                        inner.reject_all("disconnected").await;
                        break;
                    }
                    TransportEvent::HeartbeatDead | TransportEvent::Reconnected => {}
                }
            }
        });

        Self(inner)
    }

    /// Send a connection-level request and await its response.
    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<Value, RequestError> {
        send_request(&self.0.transport, &self.0.root, None, method.into(), params).await
    }

    /// Send a connection-level notification (fire-and-forget).
    pub fn notify(&self, method: impl Into<String>, params: Option<Value>) {
        let notif = WireNotification {
            stream: None,
            method: method.into(),
            params,
        };
        if let Ok(bytes) = encode_frame(&notif) {
            self.0.transport.send(bytes);
        }
    }

    /// Subscribe to a connection-level notification.
    pub async fn on<F>(&self, method: impl Into<String>, handler: F)
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let mut map = self.0.root.notification_handlers.lock().await;
        map.entry(method.into())
            .or_default()
            .push(Arc::new(handler));
    }

    /// Register a connection-level request handler.
    pub async fn handle_request<F, Fut>(&self, method: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, WireError>> + Send + 'static,
    {
        let arc: RequestHandler = Arc::new(move |params| Box::pin(handler(params)));
        let mut map = self.0.root.request_handlers.lock().await;
        map.insert(method.into(), arc);
    }

    /// Typed connection-level request keyed by an [`RpcMethod`] impl.
    pub async fn request_typed<M>(
        &self,
        params: &M::Params,
    ) -> Result<M::Result, TypedRequestError>
    where
        M: RpcMethod,
    {
        let value = to_msgpack_value(params).map_err(TypedRequestError::EncodeParams)?;
        let result_value = self.request(M::METHOD, Some(value)).await?;
        from_msgpack_value::<M::Result>(result_value)
            .map_err(|e| TypedRequestError::DecodeResult(e.to_string()))
    }

    /// Typed connection-level notification.
    pub fn notify_typed<T>(&self, method: impl Into<String>, params: &T)
    where
        T: Serialize,
    {
        let Ok(value) = to_msgpack_value(params) else {
            return;
        };
        self.notify(method, Some(value));
    }

    /// Subscribe to a connection-level notification, decoding params into `T`.
    pub async fn on_typed<T, F>(&self, method: impl Into<String>, handler: F)
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T) + Send + Sync + 'static,
    {
        self.on(method, move |value| {
            if let Ok(decoded) = from_msgpack_value::<T>(value) {
                handler(decoded);
            }
        })
        .await;
    }

    /// Register a typed connection-level request handler.
    pub async fn handle_request_typed<M, F, Fut>(&self, handler: F)
    where
        M: RpcMethod,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
        F: Fn(M::Params) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<M::Result, WireError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.handle_request(M::METHOD, move |value| {
            let handler = handler.clone();
            async move {
                let params: M::Params = from_msgpack_value(value).map_err(|e| WireError {
                    code: -32602,
                    message: format!("invalid params: {e}"),
                    data: None,
                })?;
                let result = handler(params).await?;
                to_msgpack_value(&result).map_err(|e| WireError {
                    code: -32603,
                    message: format!("encode result: {e}"),
                    data: None,
                })
            }
        })
        .await;
    }

    /// Open (or look up) a virtual stream.
    pub fn stream(&self, id: impl Into<String>) -> Stream {
        let id = id.into();
        let inner = self.0.clone();
        let state = {
            let mut streams = inner.streams.lock().expect("streams mutex poisoned");
            streams
                .entry(id.clone())
                .or_insert_with(ChannelState::new)
                .clone()
        };
        Stream {
            id,
            transport: inner.transport.clone(),
            state,
            owner: inner,
        }
    }

    /// Async alias for [`Connection::stream`] — kept for symmetry with the
    /// TypeScript API.
    pub async fn stream_async(&self, id: impl Into<String>) -> Stream {
        self.stream(id)
    }

    /// Close a virtual stream, rejecting any pending requests on it.
    pub async fn close_stream(&self, id: &str) {
        let removed = {
            let mut streams = self.0.streams.lock().expect("streams mutex poisoned");
            streams.remove(id)
        };
        if let Some(state) = removed {
            state.reject_all_pending("stream closed").await;
        }
    }

    /// Close the connection and underlying transport.
    pub async fn close(&self) {
        self.0.reject_all("connection closed").await;
        self.0.transport.close();
    }

    /// Access the underlying transport (for heartbeat control etc.).
    pub fn transport(&self) -> &Transport {
        &self.0.transport
    }
}

impl ConnectionInner {
    async fn reject_all(&self, reason: &str) {
        self.root.reject_all_pending(reason).await;
        let streams = {
            let mut map = self.streams.lock().expect("streams mutex poisoned");
            std::mem::take(&mut *map)
        };
        for state in streams.into_values() {
            state.reject_all_pending(reason).await;
        }
    }

    async fn handle_frame(self: &Arc<Self>, bytes: Vec<u8>) {
        let value: Value = match rmp_serde::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return,
        };
        // Inspect for stream tag.
        let stream_id = value
            .as_map()
            .and_then(|m| {
                m.iter().find_map(|(k, v)| {
                    if k.as_str() == Some("stream") {
                        v.as_str().map(str::to_string)
                    } else {
                        None
                    }
                })
            });

        let msg = value_to_wire_message(value);
        let sender = SendFn {
            transport: self.transport.clone(),
        };

        if let Some(sid) = stream_id {
            let state = {
                let map = self.streams.lock().expect("streams mutex poisoned");
                map.get(&sid).cloned()
            };
            if let Some(state) = state {
                state.dispatch(msg, sender).await;
            }
            return;
        }

        self.root.dispatch(msg, sender).await;
    }
}

// ── Stream ──────────────────────────────────────────────────────────

/// A virtual multiplexed channel within a [`Connection`].
#[derive(Clone)]
pub struct Stream {
    id: String,
    transport: Transport,
    state: Arc<ChannelState>,
    owner: Arc<ConnectionInner>,
}

impl Stream {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<Value, RequestError> {
        send_request(
            &self.transport,
            &self.state,
            Some(self.id.clone()),
            method.into(),
            params,
        )
        .await
    }

    pub fn notify(&self, method: impl Into<String>, params: Option<Value>) {
        let notif = WireNotification {
            stream: Some(self.id.clone()),
            method: method.into(),
            params,
        };
        if let Ok(bytes) = encode_frame(&notif) {
            self.transport.send(bytes);
        }
    }

    pub async fn on<F>(&self, method: impl Into<String>, handler: F)
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let mut map = self.state.notification_handlers.lock().await;
        map.entry(method.into())
            .or_default()
            .push(Arc::new(handler));
    }

    pub async fn handle_request<F, Fut>(&self, method: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, WireError>> + Send + 'static,
    {
        let arc: RequestHandler = Arc::new(move |params| Box::pin(handler(params)));
        let mut map = self.state.request_handlers.lock().await;
        map.insert(method.into(), arc);
    }

    /// Typed stream-level request keyed by an [`RpcMethod`] impl.
    pub async fn request_typed<M>(
        &self,
        params: &M::Params,
    ) -> Result<M::Result, TypedRequestError>
    where
        M: RpcMethod,
    {
        let value = to_msgpack_value(params).map_err(TypedRequestError::EncodeParams)?;
        let result_value = self.request(M::METHOD, Some(value)).await?;
        from_msgpack_value::<M::Result>(result_value)
            .map_err(|e| TypedRequestError::DecodeResult(e.to_string()))
    }

    /// Typed stream-level notification.
    pub fn notify_typed<T>(&self, method: impl Into<String>, params: &T)
    where
        T: Serialize,
    {
        let Ok(value) = to_msgpack_value(params) else {
            return;
        };
        self.notify(method, Some(value));
    }

    /// Subscribe to a stream-level notification, decoding params into `T`.
    pub async fn on_typed<T, F>(&self, method: impl Into<String>, handler: F)
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T) + Send + Sync + 'static,
    {
        self.on(method, move |value| {
            if let Ok(decoded) = from_msgpack_value::<T>(value) {
                handler(decoded);
            }
        })
        .await;
    }

    /// Register a typed stream-level request handler.
    pub async fn handle_request_typed<M, F, Fut>(&self, handler: F)
    where
        M: RpcMethod,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
        F: Fn(M::Params) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<M::Result, WireError>> + Send + 'static,
    {
        let handler = Arc::new(handler);
        self.handle_request(M::METHOD, move |value| {
            let handler = handler.clone();
            async move {
                let params: M::Params = from_msgpack_value(value).map_err(|e| WireError {
                    code: -32602,
                    message: format!("invalid params: {e}"),
                    data: None,
                })?;
                let result = handler(params).await?;
                to_msgpack_value(&result).map_err(|e| WireError {
                    code: -32603,
                    message: format!("encode result: {e}"),
                    data: None,
                })
            }
        })
        .await;
    }

    /// Close this stream. Other streams and the connection stay open.
    pub fn close(&self) {
        let removed = {
            let mut streams = self
                .owner
                .streams
                .lock()
                .expect("streams mutex poisoned");
            streams.remove(&self.id)
        };
        if let Some(state) = removed {
            tokio::spawn(async move {
                state.reject_all_pending("stream closed").await;
            });
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

async fn send_request(
    transport: &Transport,
    state: &Arc<ChannelState>,
    stream: Option<String>,
    method: String,
    params: Option<Value>,
) -> Result<Value, RequestError> {
    let id = Uuid::new_v4().to_string();
    let req = WireRequest {
        stream,
        id: Value::String(id.clone().into()),
        method,
        params,
    };

    let (tx, rx) = oneshot::channel();
    {
        let mut pending = state.pending.lock().await;
        pending.insert(id.clone(), tx);
    }

    let bytes = encode_frame(&req).map_err(|e| {
        RequestError::Wire(WireError {
            code: -32000,
            message: format!("encode error: {e}"),
            data: None,
        })
    })?;
    transport.send(bytes);

    match rx.await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(RequestError::Wire(err)),
        Err(_) => Err(RequestError::Closed),
    }
}
