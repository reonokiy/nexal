//! Sprites.dev backend PoC.
//!
//! This backend maps Nexal's container lifecycle onto a persistent Sprite:
//! create/reuse a Sprite, upload `nexal-agent`, start it inside the Sprite,
//! then expose a local loopback TCP port that tunnels bytes through Sprites'
//! WebSocket TCP proxy to the in-Sprite agent port.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, warn};

use super::{BackendError, ContainerBackend, ContainerHandle, ContainerSpec};

const DEFAULT_API_BASE: &str = "https://api.sprites.dev";
const DEFAULT_AGENT_PORT: u16 = 9100;
const DEFAULT_AGENT_BIN: &str = "/usr/local/bin/nexal-agent";

#[derive(Debug, Clone)]
pub struct SpritesConfig {
    pub token: String,
    pub api_base: Option<String>,
    pub name_prefix: Option<String>,
    pub agent_port: Option<u16>,
    pub agent_bin_path: Option<String>,
}

pub struct SpritesBackend {
    http: Client,
    token: String,
    api_base: String,
    name_prefix: Option<String>,
    agent_port: u16,
    agent_bin: String,
    tunnels: Arc<Mutex<HashMap<String, TunnelHandle>>>,
}

struct TunnelHandle {
    local_addr: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(serde::Deserialize)]
struct SpriteSummary {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

impl SpritesBackend {
    pub fn new(cfg: SpritesConfig) -> Result<Self, BackendError> {
        if cfg.token.is_empty() {
            return Err(BackendError::Io(
                "sprites backend requires backend.sprites_token".into(),
            ));
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| BackendError::Io(format!("build sprites http client: {e}")))?;
        Ok(Self {
            http,
            token: cfg.token,
            api_base: cfg
                .api_base
                .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
                .trim_end_matches('/')
                .to_string(),
            name_prefix: cfg.name_prefix,
            agent_port: cfg.agent_port.unwrap_or(DEFAULT_AGENT_PORT),
            agent_bin: cfg
                .agent_bin_path
                .unwrap_or_else(|| DEFAULT_AGENT_BIN.to_string()),
            tunnels: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn sprite_name(&self, spec_name: &str) -> String {
        match &self.name_prefix {
            Some(prefix) => format!("{prefix}{spec_name}"),
            None => spec_name.to_string(),
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.api_base, path)
    }

    fn ws_api_base(&self) -> String {
        if let Some(rest) = self.api_base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = self.api_base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            self.api_base.clone()
        }
    }

    async fn api(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, String), BackendError> {
        let mut req = self
            .http
            .request(method, self.api_url(path))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            req = req
                .header("content-type", "application/json")
                .body(serde_json::to_string(&b).unwrap_or_default());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BackendError::Io(format!("sprites api {path}: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BackendError::Io(format!("sprites api {path}: read body: {e}")))?;
        Ok((status, text))
    }

    async fn sprite_exists(&self, name: &str) -> Result<bool, BackendError> {
        let (status, body) = self
            .api(Method::GET, &format!("/v1/sprites/{name}"), None)
            .await?;
        match status {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => Err(BackendError::Cli(format!(
                "get sprite {name}: {status} {body}"
            ))),
        }
    }

    async fn create_sprite(&self, name: &str, spec: &ContainerSpec) -> Result<(), BackendError> {
        let mut env = serde_json::Map::new();
        for (k, v) in &spec.env {
            env.insert(k.clone(), Value::String(v.clone()));
        }
        let body = json!({
            "name": name,
            "wait_for_capacity": true,
        });
        if !env.is_empty() {
            debug!(
                "sprites backend PoC ignores {} env var(s) at create time for {name}",
                env.len()
            );
        }
        let (status, text) = self.api(Method::POST, "/v1/sprites", Some(body)).await?;
        if status.is_success() || status == StatusCode::CONFLICT {
            return Ok(());
        }
        Err(BackendError::Cli(format!(
            "create sprite {name}: {status} {text}"
        )))
    }

    async fn wait_ready(&self, name: &str) -> Result<(), BackendError> {
        for _ in 0..120u32 {
            let (status, body) = self
                .api(Method::GET, &format!("/v1/sprites/{name}"), None)
                .await?;
            if status == StatusCode::NOT_FOUND {
                sleep(Duration::from_millis(500)).await;
                continue;
            }
            if !status.is_success() {
                return Err(BackendError::Cli(format!(
                    "get sprite {name}: {status} {body}"
                )));
            }
            let sprite: SpriteSummary = serde_json::from_str(&body).unwrap_or(SpriteSummary {
                name: Some(name.to_string()),
                status: None,
            });
            let ready = matches!(
                sprite.status.as_deref(),
                None | Some("ready") | Some("running") | Some("started") | Some("warm")
            );
            if ready {
                if let Some(returned_name) = sprite.name.as_deref() {
                    debug!("sprite {name} ready as {returned_name}");
                }
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
        Err(BackendError::PortDiscovery(format!(
            "sprite {name}: timed out waiting for ready"
        )))
    }

    async fn upload_agent(&self, name: &str, agent_bin: &PathBuf) -> Result<(), BackendError> {
        let bytes = tokio::fs::read(agent_bin)
            .await
            .map_err(|e| BackendError::Io(format!("read agent binary {agent_bin:?}: {e}")))?;
        let url = self.api_url(&format!("/v1/sprites/{name}/fs/write"));
        let resp = self
            .http
            .put(url)
            .bearer_auth(&self.token)
            .query(&[
                ("path", self.agent_bin.as_str()),
                ("workingDir", "/"),
                ("mode", "0755"),
                ("mkdir", "true"),
            ])
            .body(bytes)
            .send()
            .await
            .map_err(|e| BackendError::Io(format!("sprites fs/write {name}: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| BackendError::Io(format!("sprites fs/write {name}: read body: {e}")))?;
        if status.is_success() {
            return Ok(());
        }
        Err(BackendError::Cli(format!(
            "upload nexal-agent to sprite {name}: {status} {body}"
        )))
    }

    async fn exec_shell(&self, name: &str, script: &str) -> Result<(), BackendError> {
        let url = self.api_url(&format!("/v1/sprites/{name}/exec"));
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .query(&[
                ("cmd", "/bin/sh"),
                ("cmd", "-lc"),
                ("cmd", script),
                ("dir", "/"),
                ("max_run_after_disconnect", "30s"),
            ])
            .send()
            .await
            .map_err(|e| BackendError::Io(format!("sprites exec {name}: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| BackendError::Io(format!("sprites exec {name}: read body: {e}")))?;
        if status.is_success() {
            return Ok(());
        }
        Err(BackendError::Cli(format!(
            "exec in sprite {name}: {status} {body}"
        )))
    }

    async fn start_agent(&self, name: &str) -> Result<(), BackendError> {
        self.exec_shell(name, "mkdir -p /workspace /run/nexal/proxy")
            .await?;
        let body = json!({
            "cmd": self.agent_bin,
            "args": ["--listen", format!("ws://0.0.0.0:{}", self.agent_port)],
            "dir": "/workspace",
            "needs": [],
        });
        let (status, text) = self
            .api(
                Method::PUT,
                &format!("/v1/sprites/{name}/services/nexal-agent"),
                Some(body),
            )
            .await?;
        if !status.is_success() {
            return Err(BackendError::Cli(format!(
                "create nexal-agent service in sprite {name}: {status} {text}"
            )));
        }

        let (status, text) = self
            .api(
                Method::POST,
                &format!("/v1/sprites/{name}/services/nexal-agent/start"),
                None,
            )
            .await?;
        if status.is_success() {
            return Ok(());
        }
        Err(BackendError::Cli(format!(
            "start nexal-agent service in sprite {name}: {status} {text}"
        )))
    }

    async fn stop_agent(&self, name: &str) -> Result<(), BackendError> {
        let (status, text) = self
            .api(
                Method::POST,
                &format!("/v1/sprites/{name}/services/nexal-agent/stop"),
                None,
            )
            .await?;
        if status.is_success() || status == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(BackendError::Cli(format!(
            "stop nexal-agent service in sprite {name}: {status} {text}"
        )))
    }

    async fn local_tunnel_url(&self, name: &str) -> Result<String, BackendError> {
        let mut tunnels = self.tunnels.lock().await;
        if let Some(existing) = tunnels.get(name) {
            return Ok(format!("ws://{}", existing.local_addr));
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| BackendError::Io(format!("bind sprites tunnel listener: {e}")))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| BackendError::Io(format!("sprites tunnel local_addr: {e}")))?
            .to_string();
        let sprite = name.to_string();
        let token = self.token.clone();
        let ws_base = self.ws_api_base();
        let port = self.agent_port;
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let sprite = sprite.clone();
                let token = token.clone();
                let ws_base = ws_base.clone();
                tokio::spawn(async move {
                    if let Err(err) = proxy_one(stream, &ws_base, &token, &sprite, port).await {
                        warn!("sprites tunnel {sprite}:{port}: {err}");
                    }
                });
            }
        });
        tunnels.insert(
            name.to_string(),
            TunnelHandle {
                local_addr: local_addr.clone(),
                task,
            },
        );
        Ok(format!("ws://{local_addr}"))
    }
}

#[async_trait]
impl ContainerBackend for SpritesBackend {
    fn name(&self) -> &'static str {
        "sprites"
    }

    async fn ensure(&self, spec: ContainerSpec) -> Result<ContainerHandle, BackendError> {
        let name = self.sprite_name(&spec.name);
        if !self.sprite_exists(&name).await? {
            self.create_sprite(&name, &spec).await?;
        }
        self.wait_ready(&name).await?;
        self.stop_agent(&name).await?;
        self.upload_agent(&name, &spec.agent_bin).await?;
        self.start_agent(&name).await?;
        let url = self.local_tunnel_url(&name).await?;
        Ok(ContainerHandle {
            name,
            url,
            port_map: HashMap::new(),
        })
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        self.tunnels.lock().await.remove(name);
        let (status, body) = self
            .api(Method::DELETE, &format!("/v1/sprites/{name}"), None)
            .await?;
        if status.is_success() || status == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(BackendError::Cli(format!(
            "delete sprite {name}: {status} {body}"
        )))
    }

    async fn exists(&self, name: &str) -> Result<bool, BackendError> {
        self.sprite_exists(name).await
    }

    async fn url(&self, name: &str) -> Result<String, BackendError> {
        self.local_tunnel_url(name).await
    }
}

async fn proxy_one(
    tcp: TcpStream,
    ws_base: &str,
    token: &str,
    sprite: &str,
    port: u16,
) -> Result<(), BackendError> {
    let url = format!("{ws_base}/v1/sprites/{sprite}/proxy");
    let mut req = url
        .as_str()
        .into_client_request()
        .map_err(|e| BackendError::Io(format!("build sprites proxy request: {e}")))?;
    let auth = format!("Bearer {token}")
        .parse()
        .map_err(|e| BackendError::Io(format!("build sprites proxy auth header: {e}")))?;
    req.headers_mut().insert(AUTHORIZATION, auth);
    let (ws, _) = connect_async(req)
        .await
        .map_err(|e| BackendError::Io(format!("connect sprites proxy: {e}")))?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    ws_tx
        .send(Message::Text(
            json!({ "host": "127.0.0.1", "port": port })
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| BackendError::Io(format!("sprites proxy init: {e}")))?;

    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    if let Ok(Some(first)) = timeout(Duration::from_millis(500), ws_rx.next()).await {
        match first.map_err(|e| BackendError::Io(format!("sprites proxy init response: {e}")))? {
            Message::Text(text) => {
                debug!("sprites proxy init response for {sprite}:{port}: {text}");
            }
            Message::Binary(bytes) => tcp_tx
                .write_all(&bytes)
                .await
                .map_err(|e| BackendError::Io(format!("sprites proxy initial bytes: {e}")))?,
            Message::Close(_) => return Ok(()),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }

    let tcp_to_ws = async {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = tcp_rx.read(&mut buf).await?;
            if n == 0 {
                let _ = ws_tx.send(Message::Close(None)).await;
                break;
            }
            ws_tx
                .send(Message::Binary(buf[..n].to_vec().into()))
                .await?;
        }
        Result::<(), Box<dyn std::error::Error + Send + Sync>>::Ok(())
    };
    let ws_to_tcp = async {
        while let Some(msg) = ws_rx.next().await {
            match msg? {
                Message::Binary(bytes) => tcp_tx.write_all(&bytes).await?,
                Message::Text(text) => tcp_tx.write_all(text.as_bytes()).await?,
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
        Result::<(), Box<dyn std::error::Error + Send + Sync>>::Ok(())
    };
    tokio::select! {
        res = tcp_to_ws => res.map_err(|e| BackendError::Io(format!("tcp→sprites proxy: {e}")))?,
        res = ws_to_tcp => res.map_err(|e| BackendError::Io(format!("sprites proxy→tcp: {e}")))?,
    }
    Ok(())
}
