//! Podman container lifecycle management for sandbox sessions.
//!
//! Each agent session gets its own long-lived container running the codex
//! `app-server` on a WebSocket port.  The host connects via
//! [`RemoteAppServerClient`] and sends requests over the wire.  All command
//! execution happens inside the container — no bwrap needed.

use anyhow::Context;
use nexal_config::NexalConfig;
use tokio::process::Command;
use tracing::debug;
use tracing::info;
use tracing::warn;

/// A running Podman container with the codex app-server inside.
pub(crate) struct PodmanContainer {
    pub name: String,
    pub ws_port: u16,
}

impl PodmanContainer {
    /// Create and start a Podman container for the given session key.
    ///
    /// The container runs the codex app-server on a WebSocket port, with
    /// the workspace mounted at `/workspace`.
    pub async fn start(
        session_key: &str,
        config: &NexalConfig,
        app_server_bin: &str,
    ) -> anyhow::Result<Self> {
        let name = container_name(session_key);
        let port = pick_port().await?;

        // Remove any leftover container with the same name
        let _ = Command::new("podman")
            .args(["rm", "-f", &name])
            .output()
            .await;

        let network = if config.sandbox_network {
            "pasta"
        } else {
            "none"
        };

        let mut create_args = vec![
            "create".to_string(),
            "--name".to_string(),
            name.clone(),
            "--userns=keep-id".to_string(),
            "--security-opt".to_string(),
            "no-new-privileges".to_string(),
            "--cap-drop=ALL".to_string(),
            format!("--pids-limit={}", config.sandbox_pids_limit),
            format!("--memory={}", config.sandbox_memory),
            format!("--cpus={}", config.sandbox_cpus),
            format!("--network={network}"),
            // Publish the WebSocket port
            "-p".to_string(),
            format!("127.0.0.1:{port}:{port}"),
            // Mount workspace
            "-v".to_string(),
            format!("{}:/workspace", config.workspace_dir.display()),
            // Mount app-server binary (read-only)
            "-v".to_string(),
            format!("{app_server_bin}:/usr/local/bin/nexal-app-server:ro"),
        ];

        if let Some(ref runtime) = config.sandbox_runtime {
            create_args.push("--runtime".to_string());
            create_args.push(runtime.clone());
        }

        // Mount skills if present
        let skills_in_workspace = config.workspace_dir.join("skills");
        if skills_in_workspace.exists() {
            // Skills might be a symlink — resolve it for podman
            let real_skills = tokio::fs::canonicalize(&skills_in_workspace)
                .await
                .unwrap_or(skills_in_workspace);
            create_args.push("-v".to_string());
            create_args.push(format!("{}:/workspace/skills:ro", real_skills.display()));
        }

        // Image + command
        create_args.push(config.sandbox_image.clone());
        create_args.push("/usr/local/bin/nexal-app-server".to_string());
        create_args.push("--listen".to_string());
        create_args.push(format!("ws://0.0.0.0:{port}"));

        debug!("podman create: {:?}", create_args);
        let output = Command::new("podman")
            .args(&create_args)
            .output()
            .await
            .context("podman create")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("podman create failed: {stderr}");
        }

        // Start the container
        let output = Command::new("podman")
            .args(["start", &name])
            .output()
            .await
            .context("podman start")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("podman start failed: {stderr}");
        }

        info!("podman container started: name={name} port={port}");

        Ok(Self {
            name,
            ws_port: port,
        })
    }

    pub fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.ws_port)
    }

    /// Force-remove the container.
    pub async fn stop(&self) {
        let output = Command::new("podman")
            .args(["rm", "-f", &self.name])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                info!("podman container removed: {}", self.name);
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                warn!("podman rm failed for {}: {stderr}", self.name);
            }
            Err(e) => {
                warn!("podman rm error for {}: {e}", self.name);
            }
        }
    }
}

fn container_name(session_key: &str) -> String {
    let sanitized: String = session_key
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    format!("nexal-sbx-{sanitized}")
}

async fn pick_port() -> anyhow::Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}
