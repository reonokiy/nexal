use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use arc_swap::ArcSwap;
use jsonrpsee::core::client::{Error as JsonRpseeError, Subscription};
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
use tokio::sync::Mutex;
use tokio::sync::watch;

use tokio::time::timeout;

use crate::ProcessId;
use crate::client_api::ExecServerClientConnectOptions;
use crate::client_api::RemoteExecServerConnectArgs;
use crate::protocol::ExecClosedNotification;
use crate::protocol::ExecExitedNotification;
use crate::protocol::ExecOutputDeltaNotification;
use crate::protocol::ExecParams;
use crate::protocol::ExecResponse;
use crate::protocol::FsCopyParams;
use crate::protocol::FsCopyResponse;
use crate::protocol::FsCreateDirectoryParams;
use crate::protocol::FsCreateDirectoryResponse;
use crate::protocol::FsGetMetadataParams;
use crate::protocol::FsGetMetadataResponse;
use crate::protocol::FsReadDirectoryParams;
use crate::protocol::FsReadDirectoryResponse;
use crate::protocol::FsReadFileParams;
use crate::protocol::FsReadFileResponse;
use crate::protocol::FsRemoveParams;
use crate::protocol::FsRemoveResponse;
use crate::protocol::FsWriteFileParams;
use crate::protocol::FsWriteFileResponse;
use crate::protocol::InitializeParams;
use crate::protocol::InitializeResponse;
use crate::protocol::ReadParams;
use crate::protocol::ReadResponse;
use crate::protocol::TerminateParams;
use crate::protocol::TerminateResponse;
use crate::protocol::WriteParams;
use crate::protocol::WriteResponse;
use crate::transport::jsonrpsee_api::ExecServerJsonRpseeApiClient;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
const SUBSCRIPTION_BUFFER_CAPACITY: usize = 16 * 1024;

impl Default for ExecServerClientConnectOptions {
    fn default() -> Self {
        Self {
            client_name: "nexal-core".to_string(),
            initialize_timeout: INITIALIZE_TIMEOUT,
        }
    }
}

impl From<RemoteExecServerConnectArgs> for ExecServerClientConnectOptions {
    fn from(value: RemoteExecServerConnectArgs) -> Self {
        Self {
            client_name: value.client_name,
            initialize_timeout: value.initialize_timeout,
        }
    }
}

impl RemoteExecServerConnectArgs {
    pub fn new(websocket_url: String, client_name: String) -> Self {
        Self {
            websocket_url,
            client_name,
            connect_timeout: CONNECT_TIMEOUT,
            initialize_timeout: INITIALIZE_TIMEOUT,
        }
    }
}

pub(crate) struct SessionState {
    wake_tx: watch::Sender<u64>,
    failure: Mutex<Option<String>>,
}

#[derive(Clone)]
pub(crate) struct Session {
    client: ExecServerClient,
    process_id: ProcessId,
    state: Arc<SessionState>,
}

struct Inner {
    client: Arc<WsClient>,
    // The remote transport delivers one shared notification stream for every
    // process on the connection. Keep a local process_id -> session registry so
    // we can turn those connection-global notifications into process wakeups
    // without making notifications the source of truth for output delivery.
    sessions: ArcSwap<HashMap<ProcessId, Arc<SessionState>>>,
    // ArcSwap makes reads cheap on the hot notification path, but writes still
    // need serialization so concurrent register/remove operations do not
    // overwrite each other's copy-on-write updates.
    sessions_write_lock: Mutex<()>,
    subscription_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for task in &self.subscription_tasks {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct ExecServerClient {
    inner: Arc<Inner>,
    init_response: Arc<InitializeResponse>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecServerError {
    #[error("timed out connecting to exec-server websocket `{url}` after {timeout:?}")]
    WebSocketConnectTimeout { url: String, timeout: Duration },
    #[error("failed to connect to exec-server websocket `{url}`: {source}")]
    WebSocketConnectJsonrpsee {
        url: String,
        #[source]
        source: JsonRpseeError,
    },
    #[error("timed out waiting for exec-server initialize handshake after {timeout:?}")]
    InitializeTimedOut { timeout: Duration },
    #[error("exec-server transport closed")]
    Closed,
    #[error("failed to serialize or deserialize exec-server JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("exec-server protocol error: {0}")]
    Protocol(String),
    #[error("exec-server rejected request ({code}): {message}")]
    Server { code: i64, message: String },
}

impl ExecServerClient {
    /// The initialize response from the exec-server, containing environment info.
    pub fn init_response(&self) -> &InitializeResponse {
        &self.init_response
    }

    pub async fn connect_websocket(
        args: RemoteExecServerConnectArgs,
    ) -> Result<Self, ExecServerError> {
        let websocket_url = args.websocket_url.clone();
        let connect_timeout = args.connect_timeout;
        let client = timeout(
            connect_timeout,
            WsClientBuilder::default()
                .connection_timeout(connect_timeout)
                .request_timeout(REQUEST_TIMEOUT)
                .max_buffer_capacity_per_subscription(SUBSCRIPTION_BUFFER_CAPACITY)
                .build(&websocket_url),
        )
        .await
        .map_err(|_| ExecServerError::WebSocketConnectTimeout {
            url: websocket_url.clone(),
            timeout: connect_timeout,
        })?
        .map_err(|source| ExecServerError::WebSocketConnectJsonrpsee {
            url: websocket_url.clone(),
            source,
        })?;

        Self::connect(Arc::new(client), args.into()).await
    }

    pub async fn initialize(
        &self,
        options: ExecServerClientConnectOptions,
    ) -> Result<InitializeResponse, ExecServerError> {
        let ExecServerClientConnectOptions {
            client_name,
            initialize_timeout,
        } = options;

        timeout(initialize_timeout, async {
            let response = self
                .inner
                .client
                .initialize(InitializeParams { client_name })
                .await?;
            self.notify_initialized().await?;
            Ok(response)
        })
        .await
        .map_err(|_| ExecServerError::InitializeTimedOut {
            timeout: initialize_timeout,
        })?
    }

    pub async fn exec(&self, params: ExecParams) -> Result<ExecResponse, ExecServerError> {
        self.inner.client.exec(params).await.map_err(Into::into)
    }

    pub async fn read(&self, params: ReadParams) -> Result<ReadResponse, ExecServerError> {
        self.inner
            .client
            .exec_read(params)
            .await
            .map_err(Into::into)
    }

    pub async fn write(
        &self,
        process_id: &ProcessId,
        chunk: Vec<u8>,
    ) -> Result<WriteResponse, ExecServerError> {
        self.inner
            .client
            .exec_write(WriteParams {
                process_id: process_id.clone(),
                chunk: chunk.into(),
            })
            .await
            .map_err(Into::into)
    }

    pub async fn terminate(
        &self,
        process_id: &ProcessId,
    ) -> Result<TerminateResponse, ExecServerError> {
        self.inner
            .client
            .terminate(TerminateParams {
                process_id: process_id.clone(),
            })
            .await
            .map_err(Into::into)
    }

    pub async fn fs_read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, ExecServerError> {
        self.inner
            .client
            .fs_read_file(params)
            .await
            .map_err(Into::into)
    }

    pub async fn fs_write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, ExecServerError> {
        self.inner
            .client
            .fs_write_file(params)
            .await
            .map_err(Into::into)
    }

    pub async fn fs_create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, ExecServerError> {
        self.inner
            .client
            .fs_create_directory(params)
            .await
            .map_err(Into::into)
    }

    pub async fn fs_get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, ExecServerError> {
        self.inner
            .client
            .fs_get_metadata(params)
            .await
            .map_err(Into::into)
    }

    pub async fn fs_read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, ExecServerError> {
        self.inner
            .client
            .fs_read_directory(params)
            .await
            .map_err(Into::into)
    }

    pub async fn fs_remove(
        &self,
        params: FsRemoveParams,
    ) -> Result<FsRemoveResponse, ExecServerError> {
        self.inner
            .client
            .fs_remove(params)
            .await
            .map_err(Into::into)
    }

    pub async fn fs_copy(&self, params: FsCopyParams) -> Result<FsCopyResponse, ExecServerError> {
        self.inner.client.fs_copy(params).await.map_err(Into::into)
    }

    /// Register a reverse proxy Unix socket inside the container.
    pub async fn proxy_register(
        &self,
        params: crate::protocol::ProxyRegisterParams,
    ) -> Result<crate::protocol::ProxyRegisterResponse, ExecServerError> {
        self.inner
            .client
            .proxy_register(params)
            .await
            .map_err(Into::into)
    }

    /// Unregister a proxy.
    pub async fn proxy_unregister(
        &self,
        params: crate::protocol::ProxyUnregisterParams,
    ) -> Result<crate::protocol::ProxyUnregisterResponse, ExecServerError> {
        self.inner
            .client
            .proxy_unregister(params)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn register_session(
        &self,
        process_id: &ProcessId,
    ) -> Result<Session, ExecServerError> {
        let state = Arc::new(SessionState::new());
        self.inner
            .insert_session(process_id, Arc::clone(&state))
            .await?;
        Ok(Session {
            client: self.clone(),
            process_id: process_id.clone(),
            state,
        })
    }

    pub(crate) async fn unregister_session(&self, process_id: &ProcessId) {
        self.inner.remove_session(process_id).await;
    }

    async fn connect(
        rpc_client: Arc<WsClient>,
        options: ExecServerClientConnectOptions,
    ) -> Result<Self, ExecServerError> {
        let output_subscription = rpc_client.subscribe_exec_output(None).await?;
        let exited_subscription = rpc_client.subscribe_exec_exited(None).await?;
        let closed_subscription = rpc_client.subscribe_exec_closed(None).await?;

        let inner = Arc::new_cyclic(|weak| Inner {
            client: rpc_client,
            sessions: ArcSwap::from_pointee(HashMap::new()),
            sessions_write_lock: Mutex::new(()),
            subscription_tasks: vec![
                spawn_notification_task(
                    weak.clone(),
                    output_subscription,
                    "process/output",
                    |inner, notification| {
                        Box::pin(handle_exec_output_notification(inner, notification))
                    },
                ),
                spawn_notification_task(
                    weak.clone(),
                    exited_subscription,
                    "process/exited",
                    |inner, notification| {
                        Box::pin(handle_exec_exited_notification(inner, notification))
                    },
                ),
                spawn_notification_task(
                    weak.clone(),
                    closed_subscription,
                    "process/closed",
                    |inner, notification| {
                        Box::pin(handle_exec_closed_notification(inner, notification))
                    },
                ),
            ],
        });

        let client = Self {
            inner,
            init_response: Arc::new(InitializeResponse::default()),
        };
        let response = client.initialize(options).await?;
        Ok(Self {
            init_response: Arc::new(response),
            ..client
        })
    }

    async fn notify_initialized(&self) -> Result<(), ExecServerError> {
        self.inner.client.initialized().await.map_err(Into::into)
    }
}

impl From<JsonRpseeError> for ExecServerError {
    fn from(value: JsonRpseeError) -> Self {
        match value {
            JsonRpseeError::Call(error) => Self::Server {
                code: i64::from(error.code()),
                message: error.message().to_string(),
            },
            JsonRpseeError::ParseError(err) => Self::Json(err),
            JsonRpseeError::Transport(_) => Self::Closed,
            JsonRpseeError::RestartNeeded(_) => Self::Closed,
            JsonRpseeError::ServiceDisconnect => Self::Closed,
            JsonRpseeError::RequestTimeout => {
                Self::Protocol("exec-server request timed out".to_string())
            }
            JsonRpseeError::InvalidSubscriptionId => {
                Self::Protocol("exec-server returned invalid subscription id".to_string())
            }
            JsonRpseeError::InvalidRequestId(err) => Self::Protocol(err.to_string()),
            JsonRpseeError::Custom(message) => Self::Protocol(message),
            JsonRpseeError::HttpNotImplemented => {
                Self::Protocol("exec-server websocket client operation not implemented".to_string())
            }
            JsonRpseeError::EmptyBatchRequest(err) => Self::Protocol(err.to_string()),
            JsonRpseeError::RegisterMethod(err) => Self::Protocol(err.to_string()),
        }
    }
}

impl SessionState {
    fn new() -> Self {
        let (wake_tx, _wake_rx) = watch::channel(0);
        Self {
            wake_tx,
            failure: Mutex::new(None),
        }
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.wake_tx.subscribe()
    }

    fn note_change(&self, seq: u64) {
        let next = (*self.wake_tx.borrow()).max(seq);
        let _ = self.wake_tx.send(next);
    }

    async fn set_failure(&self, message: String) {
        let mut failure = self.failure.lock().await;
        if failure.is_none() {
            *failure = Some(message);
        }
        drop(failure);
        let next = (*self.wake_tx.borrow()).saturating_add(1);
        let _ = self.wake_tx.send(next);
    }

    async fn failed_response(&self) -> Option<ReadResponse> {
        self.failure
            .lock()
            .await
            .clone()
            .map(|message| self.synthesized_failure(message))
    }

    fn synthesized_failure(&self, message: String) -> ReadResponse {
        let next_seq = (*self.wake_tx.borrow()).saturating_add(1);
        ReadResponse {
            chunks: Vec::new(),
            next_seq,
            exited: true,
            exit_code: None,
            closed: true,
            failure: Some(message),
        }
    }
}

impl Session {
    pub(crate) fn process_id(&self) -> &ProcessId {
        &self.process_id
    }

    pub(crate) fn subscribe_wake(&self) -> watch::Receiver<u64> {
        self.state.subscribe()
    }

    pub(crate) async fn read(
        &self,
        after_seq: Option<u64>,
        max_bytes: Option<usize>,
        wait_ms: Option<u64>,
    ) -> Result<ReadResponse, ExecServerError> {
        if let Some(response) = self.state.failed_response().await {
            return Ok(response);
        }

        match self
            .client
            .read(ReadParams {
                process_id: self.process_id.clone(),
                after_seq,
                max_bytes,
                wait_ms,
            })
            .await
        {
            Ok(response) => Ok(response),
            Err(err) if is_transport_closed_error(&err) => {
                let message = disconnected_message(/*reason*/ None);
                self.state.set_failure(message.clone()).await;
                Ok(self.state.synthesized_failure(message))
            }
            Err(err) => Err(err),
        }
    }

    pub(crate) async fn write(&self, chunk: Vec<u8>) -> Result<WriteResponse, ExecServerError> {
        self.client.write(&self.process_id, chunk).await
    }

    pub(crate) async fn terminate(&self) -> Result<(), ExecServerError> {
        self.client.terminate(&self.process_id).await?;
        Ok(())
    }

    pub(crate) async fn unregister(&self) {
        self.client.unregister_session(&self.process_id).await;
    }
}

impl Inner {
    fn get_session(&self, process_id: &ProcessId) -> Option<Arc<SessionState>> {
        self.sessions.load().get(process_id).cloned()
    }

    async fn insert_session(
        &self,
        process_id: &ProcessId,
        session: Arc<SessionState>,
    ) -> Result<(), ExecServerError> {
        let _sessions_write_guard = self.sessions_write_lock.lock().await;
        let sessions = self.sessions.load();
        if sessions.contains_key(process_id) {
            return Err(ExecServerError::Protocol(format!(
                "session already registered for process {process_id}"
            )));
        }
        let mut next_sessions = sessions.as_ref().clone();
        next_sessions.insert(process_id.clone(), session);
        self.sessions.store(Arc::new(next_sessions));
        Ok(())
    }

    async fn remove_session(&self, process_id: &ProcessId) -> Option<Arc<SessionState>> {
        let _sessions_write_guard = self.sessions_write_lock.lock().await;
        let sessions = self.sessions.load();
        let session = sessions.get(process_id).cloned();
        session.as_ref()?;
        let mut next_sessions = sessions.as_ref().clone();
        next_sessions.remove(process_id);
        self.sessions.store(Arc::new(next_sessions));
        session
    }

    async fn take_all_sessions(&self) -> HashMap<ProcessId, Arc<SessionState>> {
        let _sessions_write_guard = self.sessions_write_lock.lock().await;
        let sessions = self.sessions.load();
        let drained_sessions = sessions.as_ref().clone();
        self.sessions.store(Arc::new(HashMap::new()));
        drained_sessions
    }
}

fn disconnected_message(reason: Option<&str>) -> String {
    match reason {
        Some(reason) => format!("exec-server transport disconnected: {reason}"),
        None => "exec-server transport disconnected".to_string(),
    }
}

fn is_transport_closed_error(error: &ExecServerError) -> bool {
    matches!(error, ExecServerError::Closed)
        || matches!(
            error,
            ExecServerError::Server {
                code: -32000,
                message,
            } if message == "JSON-RPC transport closed"
        )
}

async fn fail_all_sessions(inner: &Arc<Inner>, message: String) {
    let sessions = inner.take_all_sessions().await;

    for (_, session) in sessions {
        session.set_failure(message.clone()).await;
    }
}

type NotificationHandler<T> =
    fn(Arc<Inner>, T) -> Pin<Box<dyn Future<Output = Result<(), ExecServerError>> + Send>>;

fn spawn_notification_task<T>(
    weak: Weak<Inner>,
    mut subscription: Subscription<T>,
    method: &'static str,
    handle: NotificationHandler<T>,
) -> tokio::task::JoinHandle<()>
where
    T: Send + 'static + serde::de::DeserializeOwned,
{
    tokio::spawn(async move {
        loop {
            match subscription.next().await {
                Some(Ok(notification)) => {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    if let Err(err) = handle(inner.clone(), notification).await {
                        fail_all_sessions(
                            &inner,
                            format!(
                                "exec-server notification handling failed for `{method}`: {err}"
                            ),
                        )
                        .await;
                        return;
                    }
                }
                Some(Err(err)) => {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    fail_all_sessions(
                        &inner,
                        format!("failed to decode exec-server notification `{method}`: {err}"),
                    )
                    .await;
                    return;
                }
                None => {
                    if let Some(inner) = weak.upgrade() {
                        fail_all_sessions(&inner, disconnected_message(None)).await;
                    }
                    return;
                }
            }
        }
    })
}

async fn handle_exec_output_notification(
    inner: Arc<Inner>,
    notification: ExecOutputDeltaNotification,
) -> Result<(), ExecServerError> {
    if let Some(session) = inner.get_session(&notification.process_id) {
        session.note_change(notification.seq);
    }
    Ok(())
}

async fn handle_exec_exited_notification(
    inner: Arc<Inner>,
    notification: ExecExitedNotification,
) -> Result<(), ExecServerError> {
    if let Some(session) = inner.get_session(&notification.process_id) {
        session.note_change(notification.seq);
    }
    Ok(())
}

async fn handle_exec_closed_notification(
    inner: Arc<Inner>,
    notification: ExecClosedNotification,
) -> Result<(), ExecServerError> {
    // Closed is the terminal lifecycle event for this process, so drop
    // the routing entry before forwarding it.
    let session = inner.remove_session(&notification.process_id).await;
    if let Some(session) = session {
        session.note_change(notification.seq);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::protocol::JSONRPCMessage;
    use crate::protocol::JSONRPCNotification;
    use crate::protocol::JSONRPCResponse;
    use futures::SinkExt;
    use futures::StreamExt;
    use pretty_assertions::assert_eq;
    use tokio::io::AsyncRead;
    use tokio::io::AsyncWrite;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio::time::Duration;
    use tokio::time::timeout;
    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    use super::ExecServerClient;
    use super::ExecServerClientConnectOptions;
    use super::RemoteExecServerConnectArgs;
    use crate::ProcessId;
    use crate::protocol::EXEC_EXITED_METHOD;
    use crate::protocol::EXEC_OUTPUT_DELTA_METHOD;
    use crate::protocol::ExecExitedNotification;
    use crate::protocol::ExecOutputDeltaNotification;
    use crate::protocol::ExecOutputStream;
    use crate::protocol::INITIALIZE_METHOD;
    use crate::protocol::INITIALIZED_METHOD;
    use crate::protocol::InitializeResponse;

    async fn read_jsonrpc_message<S>(websocket: &mut WebSocketStream<S>) -> JSONRPCMessage
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let frame = timeout(Duration::from_secs(1), websocket.next())
            .await
            .expect("json-rpc read should not time out")
            .expect("json-rpc websocket should stay open")
            .expect("json-rpc read should succeed");
        match frame {
            Message::Text(text) => {
                serde_json::from_str(text.as_ref()).expect("json-rpc text frame should parse")
            }
            Message::Binary(bytes) => {
                serde_json::from_slice(bytes.as_ref()).expect("json-rpc binary frame should parse")
            }
            other => panic!("expected json-rpc message frame, got {other:?}"),
        }
    }

    async fn write_jsonrpc_message<S>(websocket: &mut WebSocketStream<S>, message: JSONRPCMessage)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let encoded = serde_json::to_string(&message).expect("json-rpc message should serialize");
        websocket
            .send(Message::Text(encoded.into()))
            .await
            .expect("json-rpc message should write");
    }

    #[tokio::test]
    async fn wake_notifications_do_not_block_other_sessions() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should have local addr");
        let (notifications_tx, mut notifications_rx) = mpsc::channel(16);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("server should accept");
            let mut websocket = accept_async(stream)
                .await
                .expect("server websocket handshake should succeed");

            let mut output_subscription_id = None;
            let mut exited_subscription_id = None;
            let mut closed_subscription_id = None;

            for subscription_id in 1..=3 {
                let request = match read_jsonrpc_message(&mut websocket).await {
                    JSONRPCMessage::Request(request) => request,
                    other => panic!("expected subscription request, got {other:?}"),
                };
                match request.method.as_str() {
                    "process/subscribe_output" => output_subscription_id = Some(subscription_id),
                    "process/subscribe_exited" => exited_subscription_id = Some(subscription_id),
                    "process/subscribe_closed" => closed_subscription_id = Some(subscription_id),
                    other => panic!("unexpected subscription request method `{other}`"),
                }
                write_jsonrpc_message(
                    &mut websocket,
                    JSONRPCMessage::Response(JSONRPCResponse {
                        jsonrpc: jsonrpsee::types::TwoPointZero,
                        id: request.id,
                        result: serde_json::json!(subscription_id),
                    }),
                )
                .await;
            }

            assert_eq!(output_subscription_id, Some(1));
            assert_eq!(exited_subscription_id, Some(2));
            assert_eq!(closed_subscription_id, Some(3));

            let initialize = read_jsonrpc_message(&mut websocket).await;
            let request = match initialize {
                JSONRPCMessage::Request(request) if request.method == INITIALIZE_METHOD => request,
                other => panic!("expected initialize request, got {other:?}"),
            };
            write_jsonrpc_message(
                &mut websocket,
                JSONRPCMessage::Response(JSONRPCResponse {
                    jsonrpc: jsonrpsee::types::TwoPointZero,
                    id: request.id,
                    result: serde_json::to_value(InitializeResponse::default())
                        .expect("initialize response should serialize"),
                }),
            )
            .await;

            let initialized = read_jsonrpc_message(&mut websocket).await;
            match initialized {
                JSONRPCMessage::Request(request) if request.method == INITIALIZED_METHOD => {
                    write_jsonrpc_message(
                        &mut websocket,
                        JSONRPCMessage::Response(JSONRPCResponse {
                            jsonrpc: jsonrpsee::types::TwoPointZero,
                            id: request.id,
                            result: serde_json::Value::Null,
                        }),
                    )
                    .await;
                }
                other => panic!("expected initialized notification, got {other:?}"),
            }

            while let Some(message) = notifications_rx.recv().await {
                write_jsonrpc_message(&mut websocket, message).await;
            }
        });

        let client = ExecServerClient::connect_websocket(RemoteExecServerConnectArgs {
            websocket_url: format!("ws://{addr}"),
            client_name: ExecServerClientConnectOptions::default().client_name,
            connect_timeout: Duration::from_secs(1),
            initialize_timeout: Duration::from_secs(1),
        })
        .await
        .expect("client should connect");

        let noisy_process_id = ProcessId::from("noisy");
        let quiet_process_id = ProcessId::from("quiet");
        let _noisy_session = client
            .register_session(&noisy_process_id)
            .await
            .expect("noisy session should register");
        let quiet_session = client
            .register_session(&quiet_process_id)
            .await
            .expect("quiet session should register");
        let mut quiet_wake_rx = quiet_session.subscribe_wake();

        for seq in 0..=4096 {
            notifications_tx
                .send(JSONRPCMessage::Notification(JSONRPCNotification {
                    jsonrpc: jsonrpsee::types::TwoPointZero,
                    method: EXEC_OUTPUT_DELTA_METHOD.to_string(),
                    params: Some(serde_json::json!({
                        "subscription": 1,
                        "result": ExecOutputDeltaNotification {
                            process_id: noisy_process_id.clone(),
                            seq,
                            stream: ExecOutputStream::Stdout,
                            chunk: b"x".to_vec().into(),
                        }
                    })),
                }))
                .await
                .expect("output notification should queue");
        }

        notifications_tx
            .send(JSONRPCMessage::Notification(JSONRPCNotification {
                jsonrpc: jsonrpsee::types::TwoPointZero,
                method: EXEC_EXITED_METHOD.to_string(),
                params: Some(serde_json::json!({
                    "subscription": 2,
                    "result": ExecExitedNotification {
                        process_id: quiet_process_id,
                        seq: 1,
                        exit_code: 17,
                    }
                })),
            }))
            .await
            .expect("exit notification should queue");

        timeout(Duration::from_secs(1), quiet_wake_rx.changed())
            .await
            .expect("quiet session should receive wake before timeout")
            .expect("quiet wake channel should stay open");
        assert_eq!(*quiet_wake_rx.borrow(), 1);

        drop(notifications_tx);
        drop(client);
        server.await.expect("server task should finish");
    }
}
