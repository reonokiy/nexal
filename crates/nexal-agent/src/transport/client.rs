use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::sync::watch;
use tokio::time::timeout;
use tracing::warn;
use wtransport::{ClientConfig, Endpoint};

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
use crate::protocol::InitializeResponse;
use crate::protocol::ReadParams;
use crate::protocol::ReadResponse;
use crate::protocol::TerminateParams;
use crate::protocol::TerminateResponse;
use crate::protocol::WriteParams;
use crate::protocol::WriteResponse;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);

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
    pub fn new(url: String, client_name: String) -> Self {
        Self {
            url,
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

/// JSON-RPC invoke over newline-delimited JSON transport.
struct RpcClient {
    write_tx: tokio::sync::mpsc::Sender<Value>,
    pending: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, ExecServerError>>>>>,
    next_id: Arc<Mutex<u64>>,
    closed: Arc<Mutex<bool>>,
}

struct Inner {
    rpc: RpcClient,
    sessions: Arc<ArcSwap<HashMap<ProcessId, Arc<SessionState>>>>,
    sessions_write_lock: Mutex<()>,
    reader_task: tokio::task::JoinHandle<()>,
    transport_tasks: Vec<tokio::task::JoinHandle<()>>,
    /// Keep the QUIC endpoint + session alive so the underlying
    /// connection isn't torn down when the local variables in
    /// `connect_webtransport` go out of scope.
    _quic_handles: Vec<Box<dyn std::any::Any + Send + Sync>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reader_task.abort();
        for task in &self.transport_tasks {
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
    #[error("timed out connecting to exec-server `{url}` after {timeout:?}")]
    ConnectTimeout { url: String, timeout: Duration },
    #[error("failed to connect to exec-server `{url}`: {reason}")]
    Connect { url: String, reason: String },
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
    pub fn init_response(&self) -> &InitializeResponse {
        &self.init_response
    }

    pub async fn connect_webtransport(
        args: RemoteExecServerConnectArgs,
    ) -> Result<Self, ExecServerError> {
        let url = args.url.clone();
        let connect_timeout = args.connect_timeout;

        let config = ClientConfig::builder()
            .with_bind_default()
            .with_no_cert_validation()
            .keep_alive_interval(Some(std::time::Duration::from_secs(15)))
            .max_idle_timeout(Some(std::time::Duration::from_secs(300)))
            .expect("valid idle timeout")
            .build();
        let endpoint = Endpoint::client(config).map_err(|e| ExecServerError::Connect {
            url: url.clone(),
            reason: format!("create endpoint: {e}"),
        })?;

        let session = timeout(connect_timeout, endpoint.connect(&url))
            .await
            .map_err(|_| ExecServerError::ConnectTimeout {
                url: url.clone(),
                timeout: connect_timeout,
            })?
            .map_err(|e| ExecServerError::Connect {
                url: url.clone(),
                reason: format!("{e}"),
            })?;

        let opening = session.open_bi().await.map_err(|e| ExecServerError::Connect {
            url: url.clone(),
            reason: format!("open bi: {e}"),
        })?;
        let streams = opening.await.map_err(|e| ExecServerError::Connect {
            url: url.clone(),
            reason: format!("await bi: {e}"),
        })?;
        let bi: wtransport::stream::BiStream = streams.into();
        let conn = JsonMessageConnection::<Value>::from_webtransport(
            bi,
            format!("exec-client {url}"),
        );

        // Keep endpoint + session alive so the QUIC connection persists.
        let handles: Vec<Box<dyn std::any::Any + Send + Sync>> = vec![
            Box::new(endpoint),
            Box::new(session),
        ];
        Self::from_conn(conn, args.into(), handles).await
    }

    /// Connect over a legacy WebSocket (for backward compat / tests).
    pub async fn connect_websocket(
        args: RemoteExecServerConnectArgs,
    ) -> Result<Self, ExecServerError> {
        let url = args.url.clone();
        let connect_timeout = args.connect_timeout;

        let (ws, _) = timeout(
            connect_timeout,
            tokio_tungstenite::connect_async(&url),
        )
        .await
        .map_err(|_| ExecServerError::ConnectTimeout {
            url: url.clone(),
            timeout: connect_timeout,
        })?
        .map_err(|e| ExecServerError::Connect {
            url: url.clone(),
            reason: format!("{e}"),
        })?;

        let conn = JsonMessageConnection::<Value>::from_websocket(
            ws,
            format!("exec-client ws {url}"),
        );
        Self::from_conn(conn, args.into(), Vec::new()).await
    }

    async fn from_conn(
        conn: JsonMessageConnection<Value>,
        options: ExecServerClientConnectOptions,
        quic_handles: Vec<Box<dyn std::any::Any + Send + Sync>>,
    ) -> Result<Self, ExecServerError> {
        let (write_tx, incoming_rx, transport_tasks) = conn.into_parts();

        let pending: Arc<
            Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, ExecServerError>>>>,
        > = Arc::new(Mutex::new(HashMap::new()));
        // Single shared sessions map — both Inner and the reader task
        // must reference the same ArcSwap.
        let sessions = Arc::new(ArcSwap::from_pointee(HashMap::<ProcessId, Arc<SessionState>>::new()));

        let rpc = RpcClient {
            write_tx,
            pending: pending.clone(),
            next_id: Arc::new(Mutex::new(1u64)),
            closed: Arc::new(Mutex::new(false)),
        };

        let sessions_for_reader = sessions.clone();
        let pending_for_reader = pending.clone();
        let reader_task = tokio::spawn(async move {
            run_reader(incoming_rx, &pending_for_reader, &sessions_for_reader).await;
            drain_pending(&pending_for_reader).await;
            fail_all_sessions(&sessions_for_reader, "exec-server transport disconnected".into())
                .await;
        });

        let inner = Arc::new(Inner {
            rpc,
            sessions,
            sessions_write_lock: Mutex::new(()),
            reader_task,
            transport_tasks,
            _quic_handles: quic_handles,
        });

        let client = Self {
            inner,
            init_response: Arc::new(InitializeResponse::default()),
        };

        let response = client.do_initialize(options).await?;
        Ok(Self {
            init_response: Arc::new(response),
            ..client
        })
    }

    async fn do_initialize(
        &self,
        options: ExecServerClientConnectOptions,
    ) -> Result<InitializeResponse, ExecServerError> {
        let response: InitializeResponse = timeout(options.initialize_timeout, async {
            let r = self.invoke("initialize", json!({ "client_name": options.client_name })).await?;
            let init: InitializeResponse = serde_json::from_value(r)?;
            let _ = self.invoke("initialized", Value::Null).await?;
            Ok::<_, ExecServerError>(init)
        })
        .await
        .map_err(|_| ExecServerError::InitializeTimedOut {
            timeout: options.initialize_timeout,
        })??;
        Ok(response)
    }

    async fn invoke(&self, method: &str, params: Value) -> Result<Value, ExecServerError> {
        self.inner.rpc.invoke(method, params).await
    }

    pub async fn exec(&self, params: ExecParams) -> Result<ExecResponse, ExecServerError> {
        let v = self.invoke("process/start", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn read(&self, params: ReadParams) -> Result<ReadResponse, ExecServerError> {
        let v = self.invoke("process/read", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn write(
        &self,
        process_id: &ProcessId,
        chunk: Vec<u8>,
    ) -> Result<WriteResponse, ExecServerError> {
        let v = self
            .invoke(
                "process/write",
                serde_json::to_value(WriteParams {
                    process_id: process_id.clone(),
                    chunk: chunk.into(),
                })?,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn terminate(
        &self,
        process_id: &ProcessId,
    ) -> Result<TerminateResponse, ExecServerError> {
        let v = self
            .invoke(
                "process/terminate",
                serde_json::to_value(TerminateParams {
                    process_id: process_id.clone(),
                })?,
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, ExecServerError> {
        let v = self.invoke("fs/read_file", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, ExecServerError> {
        let v = self.invoke("fs/write_file", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, ExecServerError> {
        let v = self.invoke("fs/create_directory", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, ExecServerError> {
        let v = self.invoke("fs/get_metadata", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, ExecServerError> {
        let v = self.invoke("fs/read_directory", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_remove(
        &self,
        params: FsRemoveParams,
    ) -> Result<FsRemoveResponse, ExecServerError> {
        let v = self.invoke("fs/remove", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn fs_copy(&self, params: FsCopyParams) -> Result<FsCopyResponse, ExecServerError> {
        let v = self.invoke("fs/copy", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn proxy_register(
        &self,
        params: crate::protocol::ProxyRegisterParams,
    ) -> Result<crate::protocol::ProxyRegisterResponse, ExecServerError> {
        let v = self.invoke("proxy/register", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn proxy_unregister(
        &self,
        params: crate::protocol::ProxyUnregisterParams,
    ) -> Result<crate::protocol::ProxyUnregisterResponse, ExecServerError> {
        let v = self.invoke("proxy/unregister", serde_json::to_value(params)?).await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn initialize(
        &self,
        options: ExecServerClientConnectOptions,
    ) -> Result<InitializeResponse, ExecServerError> {
        self.do_initialize(options).await
    }

    pub(crate) async fn register_session(
        &self,
        process_id: &ProcessId,
    ) -> Result<Session, ExecServerError> {
        let state = Arc::new(SessionState::new());
        self.inner.insert_session(process_id, Arc::clone(&state)).await?;
        Ok(Session {
            client: self.clone(),
            process_id: process_id.clone(),
            state,
        })
    }

    pub(crate) async fn unregister_session(&self, process_id: &ProcessId) {
        self.inner.remove_session(process_id).await;
    }
}

impl RpcClient {
    async fn invoke(&self, method: &str, params: Value) -> Result<Value, ExecServerError> {
        tracing::debug!("rpc invoke: {method}");
        if *self.closed.lock().await {
            tracing::debug!("rpc invoke {method}: already closed!");
            return Err(ExecServerError::Closed);
        }
        let id = {
            let mut n = self.next_id.lock().await;
            let v = *n;
            *n += 1;
            v
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write_tx
            .send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .await
            .map_err(|_| ExecServerError::Closed)?;
        match rx.await {
            Ok(res) => res,
            Err(_) => Err(ExecServerError::Closed),
        }
    }
}

async fn run_reader(
    mut incoming_rx: tokio::sync::mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
    pending: &Arc<
        Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, ExecServerError>>>>,
    >,
    sessions: &Arc<ArcSwap<HashMap<ProcessId, Arc<SessionState>>>>,
) {
    while let Some(event) = incoming_rx.recv().await {
        match event {
            JsonMessageConnectionEvent::Message(value) => {
                // Response (has "id") or notification (has "method" without "id").
                if let Some(id_val) = value.get("id") {
                    if let Some(id) = id_val.as_u64() {
                        let mut map = pending.lock().await;
                        if let Some(tx) = map.remove(&id) {
                            if let Some(err) = value.get("error") {
                                let code =
                                    err.get("code").and_then(Value::as_i64).unwrap_or(-32603);
                                let msg = err
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("error")
                                    .to_string();
                                let _ = tx.send(Err(ExecServerError::Server {
                                    code,
                                    message: msg,
                                }));
                            } else {
                                let result =
                                    value.get("result").cloned().unwrap_or(Value::Null);
                                let _ = tx.send(Ok(result));
                            }
                        }
                    }
                } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                    let params = value.get("params").cloned().unwrap_or(Value::Null);
                    dispatch_notification(method, &params, sessions);
                }
            }
            JsonMessageConnectionEvent::MalformedMessage { reason } => {
                warn!("exec-client malformed message: {reason}");
            }
            JsonMessageConnectionEvent::Disconnected { .. } => break,
        }
    }
}

fn dispatch_notification(
    method: &str,
    params: &Value,
    sessions: &Arc<ArcSwap<HashMap<ProcessId, Arc<SessionState>>>>,
) {
    match method {
        "process/output" => {
            if let Ok(n) = serde_json::from_value::<ExecOutputDeltaNotification>(params.clone()) {
                if let Some(s) = sessions.load().get(&n.process_id) {
                    s.note_change(n.seq);
                }
            }
        }
        "process/exited" => {
            if let Ok(n) = serde_json::from_value::<ExecExitedNotification>(params.clone()) {
                if let Some(s) = sessions.load().get(&n.process_id) {
                    s.note_change(n.seq);
                }
            }
        }
        "process/closed" => {
            if let Ok(n) = serde_json::from_value::<ExecClosedNotification>(params.clone()) {
                if let Some(s) = sessions.load().get(&n.process_id) {
                    s.note_change(n.seq);
                }
            }
        }
        _ => {}
    }
}

async fn drain_pending(
    pending: &Arc<
        Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, ExecServerError>>>>,
    >,
) {
    let mut map = pending.lock().await;
    for (_, tx) in map.drain() {
        let _ = tx.send(Err(ExecServerError::Closed));
    }
}

async fn fail_all_sessions(
    sessions: &Arc<ArcSwap<HashMap<ProcessId, Arc<SessionState>>>>,
    message: String,
) {
    let current = sessions.load();
    for (_, state) in current.as_ref() {
        state.set_failure(message.clone()).await;
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
            Err(ExecServerError::Closed) => {
                let msg = "exec-server transport disconnected".to_string();
                self.state.set_failure(msg.clone()).await;
                Ok(self.state.synthesized_failure(msg))
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
    async fn insert_session(
        &self,
        process_id: &ProcessId,
        session: Arc<SessionState>,
    ) -> Result<(), ExecServerError> {
        let _guard = self.sessions_write_lock.lock().await;
        let current = self.sessions.load();
        if current.contains_key(process_id) {
            return Err(ExecServerError::Protocol(format!(
                "session already registered for process {process_id}"
            )));
        }
        let mut next = current.as_ref().clone();
        next.insert(process_id.clone(), session);
        self.sessions.store(Arc::new(next));
        Ok(())
    }

    async fn remove_session(&self, process_id: &ProcessId) -> Option<Arc<SessionState>> {
        let _guard = self.sessions_write_lock.lock().await;
        let current = self.sessions.load();
        let session = current.get(process_id).cloned();
        session.as_ref()?;
        let mut next = current.as_ref().clone();
        next.remove(process_id);
        self.sessions.store(Arc::new(next));
        session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The integration test for wake_notifications now requires a
    // WebTransport server or the legacy WS server. See
    // tests/exec_process.rs for E2E tests.

    #[test]
    fn connect_args_new_sets_defaults() {
        let args = RemoteExecServerConnectArgs::new(
            "https://127.0.0.1:9100".into(),
            "test".into(),
        );
        assert_eq!(args.url, "https://127.0.0.1:9100");
        assert_eq!(args.connect_timeout, CONNECT_TIMEOUT);
    }
}
