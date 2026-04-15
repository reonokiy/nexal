//! Podman backend — `podman` CLI shellouts.
//!
//! Direct port of `packages/nexal/src/sandbox/podman.ts` (the TS impl
//! that owned this responsibility before the gateway took over).
//!
//! Behaviors preserved:
//! - Container reuse by name (no `rm -f` on `ensure`).
//! - Labels: `app=nexal`, `nexal.kind=worker`, `nexal.session=<name>`,
//!   `nexal.created=<ISO time>`.
//! - HOME = `/workspace/.nexal`, NEXAL_DATA_DIR = `/workspace/.nexal`,
//!   workdir = `/workspace`.
//! - `--userns=keep-id`, `--cap-drop=ALL`, `--security-opt=no-new-privileges`.
//! - WS port 9100 inside container, published to a random host port,
//!   discovered via `podman port`.
//! - `nexal-agent` binary copied in via `podman cp`.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::warn;

use super::{BackendError, ContainerBackend, ContainerHandle, ContainerSpec};

const CONTAINER_WS_PORT: u16 = 9100;

pub struct PodmanBackend {
    podman_bin: String,
    runtime: Option<String>,
}

impl PodmanBackend {
    pub fn new(podman_bin: Option<String>, runtime: Option<String>) -> Self {
        Self {
            podman_bin: podman_bin.unwrap_or_else(|| "podman".to_string()),
            runtime,
        }
    }

    async fn podman(&self, args: &[&str]) -> Result<String, BackendError> {
        let output = Command::new(&self.podman_bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| BackendError::Io(format!("spawn podman: {e}")))?;
        if !output.status.success() {
            return Err(BackendError::Cli(format!(
                "podman {} → exit {}: {}",
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn container_exists(&self, name: &str) -> Result<bool, BackendError> {
        // `podman container exists` returns 0 when the container exists,
        // non-zero otherwise (no stderr message in either case).
        let status = Command::new(&self.podman_bin)
            .args(["container", "exists", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|e| BackendError::Io(format!("spawn podman: {e}")))?;
        Ok(status.success())
    }

    async fn discover_ws_url(&self, container_name: &str) -> Result<String, BackendError> {
        let port_arg = format!("{CONTAINER_WS_PORT}/tcp");
        for _ in 0..30u32 {
            if let Ok(out) = self.podman(&["port", container_name, &port_arg]).await {
                if let Some(line) = out.lines().next() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        return Ok(format!("ws://{trimmed}"));
                    }
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
        Err(BackendError::PortDiscovery(container_name.to_string()))
    }
}

#[async_trait]
impl ContainerBackend for PodmanBackend {
    fn name(&self) -> &'static str {
        "podman"
    }

    async fn ensure(&self, spec: ContainerSpec) -> Result<ContainerHandle, BackendError> {
        // Reuse by name if already present.
        if self.container_exists(&spec.name).await? {
            // Best-effort start; ignore "already running" failures.
            let _ = self.podman(&["start", &spec.name]).await;
            let ws_url = self.discover_ws_url(&spec.name).await?;
            return Ok(ContainerHandle {
                name: spec.name,
                ws_url,
            });
        }

        // Build `podman create` argv.
        let publish = format!("--publish=127.0.0.1::{CONTAINER_WS_PORT}/tcp");
        let mut args: Vec<String> = vec![
            "create".into(),
            "--name".into(),
            spec.name.clone(),
            "--userns=keep-id".into(),
            "--security-opt=no-new-privileges".into(),
            "--cap-drop=ALL".into(),
            // /workspace is the user-facing project area; /workspace/.nexal
            // is HOME and scratch space.
            "--env=HOME=/workspace/.nexal".into(),
            "--env=NEXAL_DATA_DIR=/workspace/.nexal".into(),
            "--workdir=/workspace".into(),
            // Default labels — frontend-supplied labels are appended below.
            "--label=app=nexal".into(),
            "--label=nexal.kind=worker".into(),
            format!("--label=nexal.session={}", spec.name),
            format!("--label=nexal.created={}", iso_now()),
            publish,
        ];

        if let Some(rt) = &self.runtime {
            args.push(format!("--runtime={rt}"));
        }
        if let Some(m) = &spec.memory {
            args.push(format!("--memory={m}"));
        }
        if let Some(c) = &spec.cpus {
            args.push(format!("--cpus={c}"));
        }
        if let Some(p) = spec.pids_limit {
            args.push(format!("--pids-limit={p}"));
        }

        // Frontend-supplied env.
        for (k, v) in &spec.env {
            args.push(format!("--env={k}={v}"));
        }

        // Frontend-supplied labels (append, do NOT overwrite our defaults).
        for (k, v) in &spec.labels {
            args.push(format!("--label={k}={v}"));
        }

        // Networking — pasta gives us a netns so the published WS port is
        // reachable from the host. The `network` flag toggles outbound DNS.
        args.push("--network=pasta".into());
        if spec.network {
            args.push("--dns=1.1.1.1".into());
            args.push("--dns=8.8.8.8".into());
        }

        if let Some(ws) = &spec.workspace {
            args.push(format!("--volume={ws}:/workspace"));
        }

        args.push(spec.image.clone());
        args.push("/usr/local/bin/nexal-agent".into());
        args.push("--listen".into());
        args.push(format!("ws://0.0.0.0:{CONTAINER_WS_PORT}"));

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.podman(&arg_refs).await?;

        // Copy the agent binary in.
        let agent_bin_str = spec
            .agent_bin
            .to_str()
            .ok_or_else(|| BackendError::Io("agent_bin path is not utf-8".into()))?;
        let dest = format!("{}:/usr/local/bin/nexal-agent", spec.name);
        self.podman(&["cp", agent_bin_str, &dest]).await?;

        self.podman(&["start", &spec.name]).await?;

        // Ensure /workspace/.nexal exists and is writable by the
        // keep-id mapped user. Done as root inside the container so it
        // works even when the image's /workspace is root-owned (the
        // common case when no host bind-mount is provided).
        //
        // When a bind-mount IS present we still mkdir+chmod — the
        // operations are no-ops if the dir already exists with the
        // right perms, and the host-side workspace is per-session so
        // any chmod we do is contained to that session's directory.
        let setup_cmd = "mkdir -p /workspace/.nexal && chmod 1777 /workspace /workspace/.nexal";
        if let Err(err) = self
            .podman(&[
                "exec",
                "--user",
                "0",
                &spec.name,
                "/bin/sh",
                "-c",
                setup_cmd,
            ])
            .await
        {
            warn!(
                "podman exec workspace setup failed for {}: {err}",
                spec.name
            );
        }

        let ws_url = self.discover_ws_url(&spec.name).await?;
        Ok(ContainerHandle {
            name: spec.name,
            ws_url,
        })
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        let _ = self.podman(&["rm", "-f", name]).await;
        Ok(())
    }

    async fn exists(&self, name: &str) -> Result<bool, BackendError> {
        self.container_exists(name).await
    }

    async fn ws_url(&self, name: &str) -> Result<String, BackendError> {
        self.discover_ws_url(name).await
    }
}

fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // RFC3339 without external chrono dep — UTC, second precision.
    let secs = now;
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Days since epoch (1970-01-01) → civil date. Algorithm by Howard Hinnant.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
