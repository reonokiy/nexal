//! Frontend server — accepts WebSocket or Unix socket connections.
//!
//! WebSocket (TCP): used for external frontend connections.
//! Unix socket: uses newline-delimited JSON over a raw stream (no WS).
//!
//! Session lifecycle:
//!   1. First client message MUST be a HMAC-signed `gateway/hello`
//!      `{ access_key, client_name, ts, nonce, signature }`.
//!   2. Once authenticated, the session can call gateway methods,
//!      `agent/invoke`, and receives `agent/notify` notifications.
//!   3. On disconnect, agent containers are detached (kept alive).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ring::hmac;

use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, UnixListener};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, warn};

use crate::protocol::{
    AgentIdParams, AgentInvokeParams, AgentNotifyParams, AgentSummary, AttachAgentParams,
    HelloParams, HelloResponse, JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    ListAgentsResponse, METHOD_AGENT_INVOKE, METHOD_ATTACH_AGENT, METHOD_DETACH_AGENT,
    METHOD_HELLO, METHOD_KILL_AGENT, METHOD_LIST_AGENTS, METHOD_REGISTER_PROXY,
    METHOD_REGISTER_STREAM_PROXY, METHOD_UNREGISTER_PROXY, METHOD_UNREGISTER_STREAM_PROXY,
    METHOD_SPAWN_AGENT, NOTIFY_AGENT, OkResponse, RegisterProxyParams, RegisterProxyResponse,
    RegisterStreamProxyParams, RegisterStreamProxyResponse, SpawnAgentParams, SpawnAgentResponse,
    UnregisterProxyParams, UnregisterStreamProxyParams, error_code, notification,
};
use crate::registry::AgentRegistry;

pub const GATEWAY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct ServerConfig {
    pub listen: String,
    pub unix: Option<PathBuf>,
    /// access_key → secret_key. Handshake is HMAC-signed.
    pub credentials: HashMap<String, String>,
    /// Shared replay guard: nonce → first-seen unix seconds.
    pub nonce_cache: Arc<Mutex<HashMap<String, i64>>>,
    pub proxy_external_base: String,
}

/// Max clock skew (seconds) tolerated between client `ts` and server.
const HELLO_MAX_SKEW_SECS: i64 = 300;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Verify a `gateway/hello` HMAC signature. Returns Ok(()) on success,
/// or an `AUTH_REJECTED` JsonRpcError describing the failure.
async fn verify_hello(cfg: &ServerConfig, p: &HelloParams) -> Result<(), JsonRpcError> {
    let reject = |msg: &str| JsonRpcError {
        code: error_code::AUTH_REJECTED,
        message: msg.into(),
        data: None,
    };

    let secret = cfg
        .credentials
        .get(&p.access_key)
        .ok_or_else(|| reject("unknown access key"))?;

    let now = unix_now();
    if (now - p.ts).abs() > HELLO_MAX_SKEW_SECS {
        return Err(reject("stale request (ts outside allowed window)"));
    }

    // Replay guard: prune old nonces, reject a reused one.
    {
        let mut cache = cfg.nonce_cache.lock().await;
        cache.retain(|_, seen| (now - *seen) <= HELLO_MAX_SKEW_SECS * 2);
        if cache.contains_key(&p.nonce) {
            return Err(reject("replayed nonce"));
        }
        cache.insert(p.nonce.clone(), now);
    }

    let sig = hex_decode(&p.signature).ok_or_else(|| reject("malformed signature"))?;
    let canonical = format!(
        "{}\n{}\n{}\n{}",
        p.access_key, p.ts, p.nonce, p.client_name
    );
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hmac::verify(&key, canonical.as_bytes(), &sig)
        .map_err(|_| reject("signature mismatch"))?;
    Ok(())
}

pub async fn serve(cfg: ServerConfig, registry: Arc<AgentRegistry>) -> std::io::Result<()> {
    if let Some(ref path) = cfg.unix {
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(path)?;
        info!("nexal-gateway listening on unix:{}", path.display());
        loop {
            let (stream, _addr) = match listener.accept().await {
                Ok(v) => v,
                Err(err) => {
                    error!("accept failed: {err}");
                    continue;
                }
            };
            let label = format!(
                "unix-{}",
                _addr
                    .as_pathname()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            );
            let cfg = cfg.clone();
            let registry = registry.clone();
            tokio::spawn(async move {
                if let Err(err) = handle_unix_stream(stream, &label, cfg, registry).await {
                    warn!("session for {label} ended: {err}");
                }
            });
        }
    } else {
        let listener = TcpListener::bind(&cfg.listen).await?;
        let local_addr = listener.local_addr()?;
        info!("nexal-gateway listening on ws://{local_addr}");

        loop {
            let (stream, remote_addr) = match listener.accept().await {
                Ok(v) => v,
                Err(err) => {
                    error!("accept failed: {err}");
                    continue;
                }
            };
            let cfg = cfg.clone();
            let registry = registry.clone();
            tokio::spawn(async move {
                let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                    Ok(ws) => ws,
                    Err(e) => {
                        warn!("websocket handshake failed for {remote_addr}: {e}");
                        return;
                    }
                };
                let label = format!("ws-{remote_addr}");
                let conn = JsonMessageConnection::<Value>::from_websocket(
                    ws_stream,
                    format!("frontend ws {label}"),
                );
                info!("frontend session opened: {label}");
                let session = Session::from_conn(conn, cfg, registry, label.clone());
                session.run().await;
                info!("frontend session closed: {label}");
            });
        }
    }
}

async fn handle_unix_stream<S>(
    stream: S,
    label: &str,
    cfg: ServerConfig,
    registry: Arc<AgentRegistry>,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let conn = JsonMessageConnection::<Value>::from_stdio(reader, writer, format!("frontend unix {label}"));
    info!("frontend session opened: {label}");
    let session = Session::from_conn(conn, cfg, registry, label.to_string());
    session.run().await;
    info!("frontend session closed: {label}");
    Ok(())
}

struct Session {
    ws_tx: mpsc::Sender<Value>,
    incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
    connection_tasks: Vec<tokio::task::JoinHandle<()>>,
    cfg: ServerConfig,
    registry: Arc<AgentRegistry>,
    authenticated: Arc<Mutex<bool>>,
}

impl Session {
    fn from_conn(
        conn: JsonMessageConnection<Value>,
        cfg: ServerConfig,
        registry: Arc<AgentRegistry>,
        _label: String,
    ) -> Self {
        let (ws_tx, incoming_rx, connection_tasks) = conn.into_parts();
        Self {
            ws_tx,
            incoming_rx,
            connection_tasks,
            cfg,
            registry,
            authenticated: Arc::new(Mutex::new(false)),
        }
    }

    async fn run(mut self) {
        let mut notify_rx = self.registry.subscribe_notifications();
        let write_tx = self.ws_tx.clone();
        let auth_for_notify = self.authenticated.clone();
        let notify_task = tokio::spawn(async move {
            while let Ok(notif) = notify_rx.recv().await {
                if !*auth_for_notify.lock().await {
                    continue;
                }
                let value = match serde_json::to_value(notification(
                    NOTIFY_AGENT,
                    serde_json::to_value(AgentNotifyParams {
                        agent_id: notif.agent_id,
                        method: notif.method,
                        params: notif.params,
                    })
                    .unwrap_or(Value::Null),
                )) {
                    Ok(value) => value,
                    Err(err) => {
                        warn!("encode notification: {err}");
                        continue;
                    }
                };
                if write_tx.send(value).await.is_err() {
                    break;
                }
            }
        });

        while let Some(event) = self.incoming_rx.recv().await {
            match event {
                JsonMessageConnectionEvent::Message(value) => self.handle_value(value).await,
                JsonMessageConnectionEvent::MalformedMessage { reason } => {
                    self.send_error(Value::Null, error_code::PARSE_ERROR, reason)
                        .await;
                }
                JsonMessageConnectionEvent::Disconnected { reason } => {
                    if let Some(reason) = reason {
                        debug!("frontend read: {reason}");
                    }
                    break;
                }
            }
        }

        notify_task.abort();
        for task in self.connection_tasks {
            task.abort();
            let _ = task.await;
        }
    }

    async fn handle_value(&self, value: Value) {
        let req: JsonRpcRequest = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(err) => {
                self.send_error(Value::Null, error_code::PARSE_ERROR, format!("json: {err}"))
                    .await;
                return;
            }
        };
        let id = req.id.clone();

        let Some(req_id) = id.clone() else {
            debug!("frontend notification ignored: {}", req.method);
            return;
        };

        if !*self.authenticated.lock().await && req.method != METHOD_HELLO {
            self.send_error(
                req_id,
                error_code::NOT_AUTHENTICATED,
                "send gateway/hello first",
            )
            .await;
            return;
        }

        let result = self.dispatch(&req).await;
        match result {
            Ok(value) => self.send_result(req_id, value).await,
            Err(err) => {
                self.send_response(JsonRpcResponse {
                    jsonrpc: JSONRPC_VERSION.into(),
                    id: req_id,
                    result: None,
                    error: Some(err),
                })
                .await
            }
        }
    }

    async fn dispatch(&self, req: &JsonRpcRequest) -> Result<Value, JsonRpcError> {
        let params = req.params.clone().unwrap_or(Value::Null);
        match req.method.as_str() {
            METHOD_HELLO => {
                let p: HelloParams = parse_params(params)?;
                verify_hello(&self.cfg, &p).await?;
                *self.authenticated.lock().await = true;
                info!("frontend client authenticated: {}", p.client_name);
                Ok(serde_json::to_value(HelloResponse {
                    ok: true,
                    gateway_version: GATEWAY_VERSION.into(),
                })
                .unwrap_or(Value::Null))
            }
            METHOD_SPAWN_AGENT => {
                let p: SpawnAgentParams = parse_params(params)?;
                let entry = self
                    .registry
                    .spawn(p.name, p.image, p.env, p.labels, p.extra_ports)
                    .await
                    .map_err(registry_err)?;
                Ok(serde_json::to_value(SpawnAgentResponse {
                    agent_id: entry.agent_id,
                    container_name: entry.container_name,
                })
                .unwrap_or(Value::Null))
            }
            METHOD_KILL_AGENT => {
                let p: AgentIdParams = parse_params(params)?;
                self.registry
                    .kill(&p.agent_id)
                    .await
                    .map_err(registry_err)?;
                Ok(serde_json::to_value(OkResponse { ok: true }).unwrap_or(Value::Null))
            }
            METHOD_DETACH_AGENT => {
                let p: AgentIdParams = parse_params(params)?;
                self.registry
                    .detach(&p.agent_id)
                    .await
                    .map_err(registry_err)?;
                Ok(serde_json::to_value(OkResponse { ok: true }).unwrap_or(Value::Null))
            }
            METHOD_ATTACH_AGENT => {
                let p: AttachAgentParams = parse_params(params)?;
                let entry = self
                    .registry
                    .attach(p.container_name)
                    .await
                    .map_err(registry_err)?;
                Ok(serde_json::to_value(SpawnAgentResponse {
                    agent_id: entry.agent_id,
                    container_name: entry.container_name,
                })
                .unwrap_or(Value::Null))
            }
            METHOD_LIST_AGENTS => {
                let entries = self.registry.list().await;
                let agents = entries
                    .into_iter()
                    .map(|e| AgentSummary {
                        agent_id: e.agent_id,
                        container_name: e.container_name,
                        created_at_unix_ms: e.created_at_unix_ms,
                    })
                    .collect();
                Ok(serde_json::to_value(ListAgentsResponse { agents }).unwrap_or(Value::Null))
            }
            METHOD_AGENT_INVOKE => {
                let p: AgentInvokeParams = parse_params(params)?;
                let entry = self
                    .registry
                    .get(&p.agent_id)
                    .await
                    .ok_or_else(|| JsonRpcError {
                        code: error_code::UNKNOWN_AGENT,
                        message: format!("no agent {}", p.agent_id),
                        data: None,
                    })?;
                entry
                    .conn
                    .invoke(&p.method, p.params)
                    .await
                    .map_err(JsonRpcError::from)
            }
            METHOD_REGISTER_PROXY => {
                let p: RegisterProxyParams = parse_params(params)?;
                let agent_entry = self.registry.get(&p.agent_id).await.ok_or_else(|| {
                    JsonRpcError {
                        code: error_code::UNKNOWN_AGENT,
                        message: format!("no agent {}", p.agent_id),
                        data: None,
                    }
                })?;
                let socket_path = container_socket_path(&p.name);
                let entry = self
                    .registry
                    .proxies
                    .register(p.agent_id.clone(), p.name.clone(), p.upstream_url, p.headers)
                    .await;
                let gateway_url = format!(
                    "{}/p/{}",
                    self.cfg.proxy_external_base.trim_end_matches('/'),
                    entry.token
                );
                let agent_resp = agent_entry
                    .conn
                    .invoke(
                        "proxy/register",
                        Some(serde_json::json!({
                            "socket_path": socket_path,
                            "upstream_url": gateway_url,
                            "headers": {},
                        })),
                    )
                    .await;
                if let Err(err) = agent_resp {
                    self.registry.proxies.unregister(&p.agent_id, &p.name).await;
                    return Err(JsonRpcError::from(err));
                }
                Ok(serde_json::to_value(RegisterProxyResponse {
                    token: entry.token,
                    socket_path,
                })
                .unwrap_or(Value::Null))
            }
            METHOD_UNREGISTER_PROXY => {
                let p: UnregisterProxyParams = parse_params(params)?;
                let socket_path = container_socket_path(&p.name);
                if let Some(agent_entry) = self.registry.get(&p.agent_id).await {
                    let _ = agent_entry
                        .conn
                        .invoke(
                            "proxy/unregister",
                            Some(serde_json::json!({ "socket_path": socket_path })),
                        )
                        .await;
                }
                let removed = self.registry.proxies.unregister(&p.agent_id, &p.name).await;
                Ok(serde_json::to_value(OkResponse { ok: removed }).unwrap_or(Value::Null))
            }
            METHOD_REGISTER_STREAM_PROXY => {
                let p: RegisterStreamProxyParams = parse_params(params)?;
                let agent_entry = self.registry.get(&p.agent_id).await.ok_or_else(|| {
                    JsonRpcError {
                        code: error_code::UNKNOWN_AGENT,
                        message: format!("no agent {}", p.agent_id),
                        data: None,
                    }
                })?;
                let upstream_addr =
                    agent_entry.port_map.get(&p.container_port).cloned().ok_or_else(|| {
                        JsonRpcError {
                            code: error_code::INVALID_PARAMS,
                            message: format!(
                                "port {} not published for agent {}",
                                p.container_port, p.agent_id
                            ),
                            data: None,
                        }
                    })?;
                let listen_addr = self
                    .registry
                    .tcp_proxies
                    .register(p.agent_id, p.name, upstream_addr)
                    .await
                    .map_err(|e| JsonRpcError {
                        code: error_code::BACKEND_ERROR,
                        message: e,
                        data: None,
                    })?;
                Ok(serde_json::to_value(RegisterStreamProxyResponse { listen_addr })
                    .unwrap_or(Value::Null))
            }
            METHOD_UNREGISTER_STREAM_PROXY => {
                let p: UnregisterStreamProxyParams = parse_params(params)?;
                let removed = self
                    .registry
                    .tcp_proxies
                    .unregister(&p.agent_id, &p.name)
                    .await;
                Ok(serde_json::to_value(OkResponse { ok: removed }).unwrap_or(Value::Null))
            }
            other => Err(JsonRpcError {
                code: error_code::METHOD_NOT_FOUND,
                message: format!("unknown method: {other}"),
                data: None,
            }),
        }
    }

    async fn send_result(&self, id: Value, result: Value) {
        self.send_response(JsonRpcResponse::ok(id, result)).await;
    }

    async fn send_error(&self, id: Value, code: i32, message: impl Into<String>) {
        self.send_response(JsonRpcResponse::err(id, code, message))
            .await;
    }

    async fn send_response(&self, resp: JsonRpcResponse) {
        let value = match serde_json::to_value(resp) {
            Ok(value) => value,
            Err(err) => {
                warn!("encode response: {err}");
                return;
            }
        };
        if let Err(err) = self.ws_tx.send(value).await {
            debug!("send response: {err}");
        }
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, JsonRpcError> {
    serde_json::from_value(value).map_err(|err| JsonRpcError {
        code: error_code::INVALID_PARAMS,
        message: format!("invalid params: {err}"),
        data: None,
    })
}

fn registry_err(err: crate::registry::RegistryError) -> JsonRpcError {
    use crate::registry::RegistryError::*;
    let (code, msg) = match err {
        Backend(e) => (error_code::BACKEND_ERROR, format!("{e}")),
        AgentConn(e) => (error_code::BACKEND_ERROR, format!("{e}")),
        UnknownAgent(id) => (error_code::UNKNOWN_AGENT, format!("unknown agent {id}")),
        UnknownContainer(name) => (
            error_code::UNKNOWN_AGENT,
            format!("unknown container {name}"),
        ),
    };
    JsonRpcError {
        code,
        message: msg,
        data: None,
    }
}

fn container_socket_path(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            sanitized.push(c);
        } else {
            sanitized.push('_');
        }
    }
    format!("/run/nexal/proxy/{sanitized}.socket")
}

#[allow(dead_code)]
fn _ensure_json_used() -> Value {
    json!(null)
}

#[cfg(test)]
mod tests {
    use super::{container_socket_path, parse_params, registry_err};
    use crate::agent_conn::AgentConnError;
    use crate::backend::BackendError;
    use crate::protocol::error_code;
    use crate::registry::RegistryError;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize)]
    struct Sample {
        agent_id: String,
        count: u32,
    }

    #[test]
    fn parse_params_succeeds_on_well_formed_json() {
        let s: Sample = parse_params(json!({ "agent_id": "a", "count": 7 }))
            .expect("well-formed params should deserialize");
        assert_eq!(s.agent_id, "a");
        assert_eq!(s.count, 7);
    }

    #[test]
    fn parse_params_wraps_serde_error_as_invalid_params() {
        let err = parse_params::<Sample>(json!({ "agent_id": "a" }))
            .expect_err("should reject missing fields");
        assert_eq!(err.code, error_code::INVALID_PARAMS);
    }

    #[test]
    fn parse_params_rejects_wrong_shape() {
        let err = parse_params::<Sample>(json!([1, 2, 3]))
            .expect_err("array is not an object");
        assert_eq!(err.code, error_code::INVALID_PARAMS);
    }

    #[test]
    fn registry_err_maps_unknown_agent_to_unknown_agent_code() {
        let err = registry_err(RegistryError::UnknownAgent("abc".into()));
        assert_eq!(err.code, error_code::UNKNOWN_AGENT);
    }

    #[test]
    fn registry_err_maps_unknown_container_to_unknown_agent_code() {
        let err = registry_err(RegistryError::UnknownContainer("nexal-x".into()));
        assert_eq!(err.code, error_code::UNKNOWN_AGENT);
    }

    #[test]
    fn registry_err_maps_backend_errors_to_backend_code() {
        let err = registry_err(RegistryError::Backend(BackendError::Cli("fail".into())));
        assert_eq!(err.code, error_code::BACKEND_ERROR);
    }

    #[test]
    fn registry_err_maps_agent_conn_errors_to_backend_code() {
        let err = registry_err(RegistryError::AgentConn(AgentConnError::BadFrame(
            "bad".into(),
        )));
        assert_eq!(err.code, error_code::BACKEND_ERROR);
    }

    #[test]
    fn socket_path_uses_convention() {
        assert_eq!(container_socket_path("jina"), "/run/nexal/proxy/jina.socket");
    }

    #[test]
    fn socket_path_preserves_safe_chars() {
        assert_eq!(
            container_socket_path("api-v2.0_final"),
            "/run/nexal/proxy/api-v2.0_final.socket"
        );
    }

    #[test]
    fn socket_path_sanitizes_unsafe_chars() {
        assert_eq!(
            container_socket_path("foo/bar baz"),
            "/run/nexal/proxy/foo_bar_baz.socket"
        );
    }

    #[test]
    fn socket_path_preserves_empty_name_safely() {
        assert_eq!(container_socket_path(""), "/run/nexal/proxy/.socket");
    }

    #[test]
    fn socket_path_sanitizes_unicode_to_underscore() {
        assert_eq!(container_socket_path("测试"), "/run/nexal/proxy/__.socket");
    }
}
