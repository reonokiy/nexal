//! Frontend WebSocket server.
//!
//! Listens on a host TCP port. Each incoming WS is a "session": one
//! application frontend (typically the nexal Bun process) talking
//! JSON-RPC 2.0.
//!
//! Session lifecycle:
//!   1. First client message MUST be `gateway/hello { token, clientName }`.
//!      Anything else gets `NOT_AUTHENTICATED`. Wrong token →
//!      `AUTH_REJECTED` and the connection is closed.
//!   2. Once authenticated, the session can call gateway methods,
//!      `agent/invoke`, and receives `agent/notify` notifications
//!      relayed from any agent.
//!   3. On disconnect, the session ends — agent containers are
//!      detached (kept alive) so the next session can re-attach them
//!      via `gateway/attachAgent`.
//!
//! Concurrency: requests on the same session are served serially in
//! the order they arrive. This keeps the response order deterministic
//! for the frontend, which simplifies its dispatcher.

use std::sync::Arc;

use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::accept_async;
use tracing::{debug, error, info, warn};

use crate::protocol::{
    AgentIdParams, AgentInvokeParams, AgentNotifyParams, AgentSummary, AttachAgentParams,
    HelloParams, HelloResponse, JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    ListAgentsResponse, METHOD_AGENT_INVOKE, METHOD_ATTACH_AGENT, METHOD_DETACH_AGENT,
    METHOD_HELLO, METHOD_KILL_AGENT, METHOD_LIST_AGENTS, METHOD_REGISTER_PROXY, METHOD_SPAWN_AGENT,
    METHOD_UNREGISTER_PROXY, NOTIFY_AGENT, OkResponse, RegisterProxyParams, RegisterProxyResponse,
    SpawnAgentParams, SpawnAgentResponse, UnregisterProxyParams, error_code, notification,
};
use crate::registry::AgentRegistry;

pub const GATEWAY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct ServerConfig {
    pub listen: String,
    pub token: String,
    pub proxy_external_base: String,
}

pub async fn serve(cfg: ServerConfig, registry: Arc<AgentRegistry>) -> std::io::Result<()> {
    let listener = TcpListener::bind(&cfg.listen).await?;
    info!("nexal-gateway listening on ws://{}", cfg.listen);
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(err) => {
                error!("accept failed: {err}");
                continue;
            }
        };
        let cfg = cfg.clone();
        let registry = registry.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, peer, cfg, registry).await {
                warn!("session for {peer} ended: {err}");
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: std::net::SocketAddr,
    cfg: ServerConfig,
    registry: Arc<AgentRegistry>,
) -> Result<(), String> {
    let ws = accept_async(stream)
        .await
        .map_err(|e| format!("ws handshake: {e}"))?;
    info!("frontend session opened: {peer}");
    let session = Session::new(ws, cfg, registry, format!("frontend websocket {peer}"));
    session.run().await;
    info!("frontend session closed: {peer}");
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
    fn new(
        ws: tokio_tungstenite::WebSocketStream<TcpStream>,
        cfg: ServerConfig,
        registry: Arc<AgentRegistry>,
        connection_label: String,
    ) -> Self {
        let (ws_tx, incoming_rx, connection_tasks) =
            JsonMessageConnection::from_websocket(ws, connection_label).into_parts();
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
                        debug!("frontend ws read: {reason}");
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
                if p.token != self.cfg.token {
                    return Err(JsonRpcError {
                        code: error_code::AUTH_REJECTED,
                        message: "invalid token".into(),
                        data: None,
                    });
                }
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
                    .spawn(p.name, p.image, p.env, p.labels, p.workspace)
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
                if self.registry.get(&p.agent_id).await.is_none() {
                    return Err(JsonRpcError {
                        code: error_code::UNKNOWN_AGENT,
                        message: format!("no agent {}", p.agent_id),
                        data: None,
                    });
                }
                let entry = self
                    .registry
                    .proxies
                    .register(p.agent_id, p.name, p.upstream_url, p.headers)
                    .await;
                let url = format!(
                    "{}/p/{}",
                    self.cfg.proxy_external_base.trim_end_matches('/'),
                    entry.token
                );
                Ok(serde_json::to_value(RegisterProxyResponse {
                    token: entry.token,
                    url,
                })
                .unwrap_or(Value::Null))
            }
            METHOD_UNREGISTER_PROXY => {
                let p: UnregisterProxyParams = parse_params(params)?;
                let removed = self.registry.proxies.unregister(&p.agent_id, &p.name).await;
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

#[allow(dead_code)]
fn _ensure_json_used() -> Value {
    json!(null)
}
