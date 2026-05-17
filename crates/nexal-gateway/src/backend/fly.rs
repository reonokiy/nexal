//! Fly.io backend — runs each agent as a Fly **Machine** via the Fly
//! Machines REST API (<https://fly.io/docs/machines/api/>).
//!
//! Design assumptions (chosen with the user):
//! - The gateway runs **inside** the same Fly app/org, so it reaches a
//!   machine over **6PN** at `private_ip:9100` (no public ports, no
//!   WireGuard). `url()` returns `https://[<ipv6>]:9100`.
//! - Machines are **persistently reused** keyed by `spec.name` (== the
//!   gateway-derived container name, used as the Fly machine `name`):
//!     * `ensure`  — find by name; `start` if stopped; else `create`.
//!     * `destroy` — stop + delete the machine.
//!   nexal's suspend/resume therefore map to machine stop/start for
//!   free (a stopped machine keeps its volume-less rootfs reset, but
//!   the name/id are reused so resume is a fast `start`).
//! - The configured image **must already ship** the agent at
//!   `agent_bin_path` (default `/usr/local/bin/nexal-agent`). Fly
//!   machines are single-container with no initContainer/host-bind, so
//!   unlike podman we cannot copy the binary in — bake it into the
//!   sandbox image (the gateway's default image already does).
//!
//! `reqwest` is pulled in without the `json` feature workspace-wide, so
//! we (de)serialize with `serde_json` + raw bodies.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::time::sleep;

use super::{BackendError, ContainerBackend, ContainerHandle, ContainerSpec};

const AGENT_WS_PORT: u16 = 9100;
const DEFAULT_API_BASE: &str = "https://api.machines.dev";
const DEFAULT_AGENT_BIN: &str = "/usr/local/bin/nexal-agent";

/// Configuration for the Fly backend, from `[backend]` in gateway.toml.
#[derive(Debug, Clone)]
pub struct FlyConfig {
    /// Fly API / org-deploy token (`Authorization: Bearer`).
    pub api_token: String,
    /// Fly app the worker machines belong to.
    pub app: String,
    /// Default region (e.g. `"iad"`). `None` → Fly picks.
    pub region: Option<String>,
    /// Machines API base. Default `https://api.machines.dev`
    /// (use `http://_api.internal:4280` from inside Fly if preferred).
    pub api_base: Option<String>,
    /// Path of `nexal-agent` inside the image. Default
    /// `/usr/local/bin/nexal-agent`.
    pub agent_bin_path: Option<String>,
}

pub struct FlyBackend {
    http: Client,
    token: String,
    app: String,
    region: Option<String>,
    api_base: String,
    agent_bin: String,
}

#[derive(serde::Deserialize)]
struct Machine {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: String,
    #[serde(default)]
    private_ip: Option<String>,
}

impl FlyBackend {
    pub fn new(cfg: FlyConfig) -> Result<Self, BackendError> {
        if cfg.api_token.is_empty() {
            return Err(BackendError::Io(
                "fly backend requires backend.fly_api_token".into(),
            ));
        }
        if cfg.app.is_empty() {
            return Err(BackendError::Io(
                "fly backend requires backend.fly_app".into(),
            ));
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| BackendError::Io(format!("build fly http client: {e}")))?;
        Ok(Self {
            http,
            token: cfg.api_token,
            app: cfg.app,
            region: cfg.region,
            api_base: cfg
                .api_base
                .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
                .trim_end_matches('/')
                .to_string(),
            agent_bin: cfg
                .agent_bin_path
                .unwrap_or_else(|| DEFAULT_AGENT_BIN.to_string()),
        })
    }

    /// One Machines API call. Returns `(status, body_text)`; maps
    /// transport errors to `BackendError::Io`.
    async fn api(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, String), BackendError> {
        let url = format!("{}/v1/apps/{}{}", self.api_base, self.app, path);
        let mut req = self.http.request(method, &url).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req
                .header("content-type", "application/json")
                .body(serde_json::to_string(&b).unwrap_or_default());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BackendError::Io(format!("fly api {path}: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BackendError::Io(format!("fly api {path}: read body: {e}")))?;
        Ok((status, text))
    }

    /// Find a machine by its Fly `name` (== `spec.name`).
    async fn find_by_name(&self, name: &str) -> Result<Option<Machine>, BackendError> {
        let (status, body) = self.api(Method::GET, "/machines", None).await?;
        if !status.is_success() {
            return Err(BackendError::Cli(format!("list machines: {status} {body}")));
        }
        let machines: Vec<Machine> = serde_json::from_str(&body)
            .map_err(|e| BackendError::Io(format!("parse machines list: {e}")))?;
        Ok(machines
            .into_iter()
            .find(|m| m.name.as_deref() == Some(name)))
    }

    async fn get_machine(&self, id: &str) -> Result<Option<Machine>, BackendError> {
        let (status, body) = self
            .api(Method::GET, &format!("/machines/{id}"), None)
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(BackendError::Cli(format!(
                "get machine {id}: {status} {body}"
            )));
        }
        let m: Machine = serde_json::from_str(&body)
            .map_err(|e| BackendError::Io(format!("parse machine {id}: {e}")))?;
        Ok(Some(m))
    }

    /// Poll until the machine is `started` with a non-empty private IP.
    async fn wait_started(&self, id: &str) -> Result<String, BackendError> {
        for _ in 0..120u32 {
            match self.get_machine(id).await? {
                Some(m) if m.state == "started" => {
                    if let Some(ip) = m.private_ip.filter(|s| !s.is_empty()) {
                        return Ok(ip);
                    }
                }
                Some(m) if matches!(m.state.as_str(), "failed" | "destroyed") => {
                    return Err(BackendError::Cli(format!(
                        "machine {id} in terminal state: {}",
                        m.state
                    )));
                }
                Some(_) => {} // created/starting/stopping — keep waiting.
                None => return Err(BackendError::Cli(format!("machine {id} disappeared"))),
            }
            sleep(Duration::from_millis(500)).await;
        }
        Err(BackendError::PortDiscovery(format!(
            "machine {id}: timed out waiting for started + private_ip"
        )))
    }

    fn handle(&self, name: String, ip: &str, spec_extra: &[u16]) -> ContainerHandle {
        let port_map = spec_extra
            .iter()
            .map(|&p| (p, format!("[{ip}]:{p}")))
            .collect();
        ContainerHandle {
            name,
            url: format!("ws://[{ip}]:{AGENT_WS_PORT}"),
            port_map,
        }
    }

    fn machine_config(&self, spec: &ContainerSpec) -> Value {
        let env: serde_json::Map<String, Value> = spec
            .env
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        json!({
            "image": spec.image,
            "env": env,
            "auto_destroy": false,
            "restart": { "policy": "no" },
            "guest": {
                "cpu_kind": "shared",
                "cpus": parse_cpus(spec.cpus.as_deref()),
                "memory_mb": parse_memory_mb(spec.memory.as_deref()),
            },
            "init": {
                "exec": [ self.agent_bin, "--listen", format!("ws://[::]:{AGENT_WS_PORT}") ]
            },
            "metadata": { "nexal": "worker", "nexal_name": spec.name }
        })
    }
}

#[async_trait]
impl ContainerBackend for FlyBackend {
    fn name(&self) -> &'static str {
        "fly"
    }

    async fn ensure(&self, spec: ContainerSpec) -> Result<ContainerHandle, BackendError> {
        // Reuse by name: start a stopped machine, return a running one.
        if let Some(m) = self.find_by_name(&spec.name).await? {
            if m.state != "started" {
                let (status, body) = self
                    .api(Method::POST, &format!("/machines/{}/start", m.id), None)
                    .await?;
                if !status.is_success() && status != StatusCode::OK {
                    return Err(BackendError::Cli(format!(
                        "start machine {}: {status} {body}",
                        m.id
                    )));
                }
            }
            let ip = self.wait_started(&m.id).await?;
            return Ok(self.handle(spec.name.clone(), &ip, &spec.extra_ports));
        }

        // Create a fresh machine.
        let mut body = json!({
            "name": spec.name,
            "config": self.machine_config(&spec),
        });
        if let Some(region) = &self.region {
            body["region"] = Value::String(region.clone());
        }
        let (status, resp) = self.api(Method::POST, "/machines", Some(body)).await?;
        if !status.is_success() {
            return Err(BackendError::Cli(format!(
                "create machine {}: {status} {resp}",
                spec.name
            )));
        }
        let created: Machine = serde_json::from_str(&resp)
            .map_err(|e| BackendError::Io(format!("parse create response: {e}")))?;
        let ip = self.wait_started(&created.id).await?;
        Ok(self.handle(spec.name, &ip, &spec.extra_ports))
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        let Some(m) = self.find_by_name(name).await? else {
            return Ok(()); // idempotent
        };
        // Stop first so delete is fast; ignore stop failures.
        let _ = self
            .api(Method::POST, &format!("/machines/{}/stop", m.id), None)
            .await;
        let (status, body) = self
            .api(
                Method::DELETE,
                &format!("/machines/{}?force=true", m.id),
                None,
            )
            .await?;
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(BackendError::Cli(format!(
                "delete machine {} ({name}): {status} {body}",
                m.id
            )))
        }
    }

    async fn exists(&self, name: &str) -> Result<bool, BackendError> {
        Ok(self.find_by_name(name).await?.is_some())
    }

    async fn url(&self, name: &str) -> Result<String, BackendError> {
        let m = self
            .find_by_name(name)
            .await?
            .ok_or_else(|| BackendError::Cli(format!("machine {name} not found")))?;
        if m.state != "started" {
            let _ = self
                .api(Method::POST, &format!("/machines/{}/start", m.id), None)
                .await;
        }
        let ip = self.wait_started(&m.id).await?;
        Ok(format!("ws://[{ip}]:{AGENT_WS_PORT}"))
    }
}

/// `"512m"` / `"1g"` / `"512"` → memory in MB (Fly wants `memory_mb`).
fn parse_memory_mb(s: Option<&str>) -> u64 {
    let s = match s {
        Some(s) => s.trim().to_lowercase(),
        None => return 512,
    };
    let (num, mult) = if let Some(n) = s.strip_suffix('g') {
        (n, 1024.0)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 1.0)
    } else {
        (s.as_str(), 1.0)
    };
    let mb = num.trim().parse::<f64>().unwrap_or(512.0) * mult;
    // Fly requires memory_mb to be a multiple of 256, min 256.
    let mb = mb.max(256.0) as u64;
    ((mb + 255) / 256) * 256
}

/// `"1.0"` / `"2"` → vCPU count (Fly `guest.cpus`, min 1).
fn parse_cpus(s: Option<&str>) -> u64 {
    s.and_then(|s| s.trim().parse::<f64>().ok())
        .map(|c| c.ceil() as u64)
        .unwrap_or(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_parsing() {
        assert_eq!(parse_memory_mb(Some("512m")), 512);
        assert_eq!(parse_memory_mb(Some("1g")), 1024);
        assert_eq!(parse_memory_mb(Some("700m")), 768); // rounded to 256x
        assert_eq!(parse_memory_mb(Some("100m")), 256); // min
        assert_eq!(parse_memory_mb(None), 512);
    }

    #[test]
    fn cpu_parsing() {
        assert_eq!(parse_cpus(Some("1.0")), 1);
        assert_eq!(parse_cpus(Some("2")), 2);
        assert_eq!(parse_cpus(Some("0.5")), 1);
        assert_eq!(parse_cpus(None), 1);
    }
}
