//! Frontend session — one per WebSocket or Unix-socket connection.
//!
//! Owns the per-connection message loop, HMAC authentication, and
//! JSON-RPC request dispatch (MessagePack binary frames).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ring::hmac;

use nexal_utils_transport::agent::AgentMethod;
use nexal_utils_transport::gateway::{
    AgentIdParams, AgentInvokeParams, AgentNotifyParams, AgentSummary, AttachAgentParams,
    GatewayMethod, HelloParams, HelloResponse, ListAgentsResponse, NOTIFY_AGENT, OkResponse,
    RegisterProxyParams, RegisterProxyResponse, RegisterStreamProxyParams,
    RegisterStreamProxyResponse, SpawnAgentParams, SpawnAgentResponse, UnregisterProxyParams,
    UnregisterStreamProxyParams,
};
use nexal_utils_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use rmpv::Value;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, info};

use crate::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, error_code, notification};
use crate::registry::AgentRegistry;

use super::{GATEWAY_VERSION, ServerConfig};

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

    {
        let mut cache = cfg.nonce_cache.lock().await;
        cache.retain(|_, seen| (now - *seen) <= HELLO_MAX_SKEW_SECS * 2);
        if cache.contains_key(&p.nonce) {
            return Err(reject("replayed nonce"));
        }
        cache.insert(p.nonce.clone(), now);
    }

    let sig = hex_decode(&p.signature).ok_or_else(|| reject("malformed signature"))?;
    let canonical = format!("{}\n{}\n{}\n{}", p.access_key, p.ts, p.nonce, p.client_name);
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hmac::verify(&key, canonical.as_bytes(), &sig).map_err(|_| reject("signature mismatch"))?;
    Ok(())
}

fn to_msgpack<T: serde::Serialize>(v: &T) -> Value {
    let mut buf = Vec::new();
    let mut ser = rmp_serde::Serializer::new(&mut buf).with_struct_map();
    v.serialize(&mut ser).unwrap();
    rmp_serde::from_slice(&buf).unwrap_or(Value::Nil)
}

pub(super) struct Session {
    ws_tx: mpsc::Sender<Value>,
    incoming_rx: mpsc::Receiver<JsonMessageConnectionEvent<Value>>,
    connection_tasks: Vec<tokio::task::JoinHandle<()>>,
    cfg: ServerConfig,
    registry: Arc<AgentRegistry>,
    authenticated: Arc<Mutex<bool>>,
}

impl Session {
    pub(super) fn from_conn(
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

    pub(super) async fn run(mut self) {
        let mut notify_rx = self.registry.subscribe_notifications();
        let write_tx = self.ws_tx.clone();
        let auth_for_notify = self.authenticated.clone();
        let notify_task = tokio::spawn(async move {
            while let Ok(notif) = notify_rx.recv().await {
                if !*auth_for_notify.lock().await {
                    continue;
                }
                let value = to_msgpack(&notification(
                    NOTIFY_AGENT,
                    to_msgpack(&AgentNotifyParams {
                        agent_id: notif.agent_id,
                        method: notif.method,
                        params: notif.params,
                    }),
                ));
                if write_tx.send(value).await.is_err() {
                    break;
                }
            }
        });

        while let Some(event) = self.incoming_rx.recv().await {
            match event {
                JsonMessageConnectionEvent::Message(value) => self.handle_value(value).await,
                JsonMessageConnectionEvent::MalformedMessage { reason } => {
                    self.send_error(Value::Nil, error_code::PARSE_ERROR, reason)
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
        let req: JsonRpcRequest = match rmpv::ext::from_value(value) {
            Ok(r) => r,
            Err(err) => {
                self.send_error(
                    Value::Nil,
                    error_code::PARSE_ERROR,
                    format!("msgpack: {err}"),
                )
                .await;
                return;
            }
        };
        let id = req.id.clone();

        let Some(req_id) = id.clone() else {
            debug!("frontend notification ignored: {}", req.method);
            return;
        };

        if !*self.authenticated.lock().await
            && GatewayMethod::parse(&req.method) != Some(GatewayMethod::Hello)
        {
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
            Ok(val) => self.send_result(req_id, val).await,
            Err(err) => {
                self.send_response(JsonRpcResponse {
                    id: req_id,
                    result: None,
                    error: Some(err),
                })
                .await
            }
        }
    }

    async fn dispatch(&self, req: &JsonRpcRequest) -> Result<Value, JsonRpcError> {
        let params = req.params.clone().unwrap_or(Value::Nil);
        match GatewayMethod::parse(&req.method) {
            Some(GatewayMethod::Hello) => {
                let p: HelloParams = super::parse_params(params)?;
                verify_hello(&self.cfg, &p).await?;
                *self.authenticated.lock().await = true;
                info!("frontend client authenticated: {}", p.client_name);
                Ok(to_msgpack(&HelloResponse {
                    ok: true,
                    gateway_version: GATEWAY_VERSION.into(),
                }))
            }
            Some(GatewayMethod::SpawnAgent) => {
                let p: SpawnAgentParams = super::parse_params(params)?;
                let entry = self
                    .registry
                    .spawn(p.name, p.image, p.env, p.labels, p.extra_ports)
                    .await
                    .map_err(super::registry_err)?;
                Ok(to_msgpack(&SpawnAgentResponse {
                    agent_id: entry.agent_id,
                    container_name: entry.container_name,
                }))
            }
            Some(GatewayMethod::KillAgent) => {
                let p: AgentIdParams = super::parse_params(params)?;
                self.registry
                    .kill(&p.agent_id)
                    .await
                    .map_err(super::registry_err)?;
                Ok(to_msgpack(&OkResponse { ok: true }))
            }
            Some(GatewayMethod::DetachAgent) => {
                let p: AgentIdParams = super::parse_params(params)?;
                self.registry
                    .detach(&p.agent_id)
                    .await
                    .map_err(super::registry_err)?;
                Ok(to_msgpack(&OkResponse { ok: true }))
            }
            Some(GatewayMethod::AttachAgent) => {
                let p: AttachAgentParams = super::parse_params(params)?;
                let entry = self
                    .registry
                    .attach(p.container_name)
                    .await
                    .map_err(super::registry_err)?;
                Ok(to_msgpack(&SpawnAgentResponse {
                    agent_id: entry.agent_id,
                    container_name: entry.container_name,
                }))
            }
            Some(GatewayMethod::ListAgents) => {
                let entries = self.registry.list().await;
                let agents = entries
                    .into_iter()
                    .map(|e| AgentSummary {
                        agent_id: e.agent_id,
                        container_name: e.container_name,
                        created_at_unix_ms: e.created_at_unix_ms,
                    })
                    .collect();
                Ok(to_msgpack(&ListAgentsResponse { agents }))
            }
            Some(GatewayMethod::AgentInvoke) => {
                let p: AgentInvokeParams = super::parse_params(params)?;
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
            Some(GatewayMethod::RegisterProxy) => {
                let p: RegisterProxyParams = super::parse_params(params)?;
                let agent_entry =
                    self.registry
                        .get(&p.agent_id)
                        .await
                        .ok_or_else(|| JsonRpcError {
                            code: error_code::UNKNOWN_AGENT,
                            message: format!("no agent {}", p.agent_id),
                            data: None,
                        })?;
                let socket_path = super::container_socket_path(&p.name);
                let entry = self
                    .registry
                    .proxies
                    .register(
                        p.agent_id.clone(),
                        p.name.clone(),
                        p.upstream_url,
                        p.headers,
                    )
                    .await;
                let gateway_url = format!(
                    "{}/p/{}",
                    self.cfg.proxy_external_base.trim_end_matches('/'),
                    entry.token
                );
                let agent_params = to_msgpack(&serde_json::json!({
                    "socket_path": socket_path,
                    "upstream_url": gateway_url,
                    "headers": {},
                }));
                let agent_resp = agent_entry
                    .conn
                    .invoke(AgentMethod::ProxyRegister.as_str(), Some(agent_params))
                    .await;
                if let Err(err) = agent_resp {
                    self.registry.proxies.unregister(&p.agent_id, &p.name).await;
                    return Err(JsonRpcError::from(err));
                }
                Ok(to_msgpack(&RegisterProxyResponse {
                    token: entry.token,
                    socket_path,
                }))
            }
            Some(GatewayMethod::UnregisterProxy) => {
                let p: UnregisterProxyParams = super::parse_params(params)?;
                let socket_path = super::container_socket_path(&p.name);
                if let Some(agent_entry) = self.registry.get(&p.agent_id).await {
                    let agent_params =
                        to_msgpack(&serde_json::json!({ "socket_path": socket_path }));
                    let _ = agent_entry
                        .conn
                        .invoke(AgentMethod::ProxyUnregister.as_str(), Some(agent_params))
                        .await;
                }
                let removed = self.registry.proxies.unregister(&p.agent_id, &p.name).await;
                Ok(to_msgpack(&OkResponse { ok: removed }))
            }
            Some(GatewayMethod::RegisterStreamProxy) => {
                let p: RegisterStreamProxyParams = super::parse_params(params)?;
                let agent_entry =
                    self.registry
                        .get(&p.agent_id)
                        .await
                        .ok_or_else(|| JsonRpcError {
                            code: error_code::UNKNOWN_AGENT,
                            message: format!("no agent {}", p.agent_id),
                            data: None,
                        })?;
                let upstream_addr = agent_entry
                    .port_map
                    .get(&p.container_port)
                    .cloned()
                    .ok_or_else(|| JsonRpcError {
                        code: error_code::INVALID_PARAMS,
                        message: format!(
                            "port {} not published for agent {}",
                            p.container_port, p.agent_id
                        ),
                        data: None,
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
                Ok(to_msgpack(&RegisterStreamProxyResponse { listen_addr }))
            }
            Some(GatewayMethod::UnregisterStreamProxy) => {
                let p: UnregisterStreamProxyParams = super::parse_params(params)?;
                let removed = self
                    .registry
                    .tcp_proxies
                    .unregister(&p.agent_id, &p.name)
                    .await;
                Ok(to_msgpack(&OkResponse { ok: removed }))
            }
            None => Err(JsonRpcError {
                code: error_code::METHOD_NOT_FOUND,
                message: format!("unknown method: {}", req.method),
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
        let value = to_msgpack(&resp);
        if let Err(err) = self.ws_tx.send(value).await {
            debug!("send response: {err}");
        }
    }
}
