//! Podman container lifecycle via the REST API socket.
//!
//! Replaces `tokio::process::Command::new("podman")` calls with HTTP
//! requests to the rootless Podman socket at
//! `$XDG_RUNTIME_DIR/podman/podman.sock`. Container creates, starts,
//! file copies, port queries, process listings, and removals all go
//! through this path — no CLI subprocess spawning.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{
    InspectContainerOptionsBuilder, RemoveContainerOptionsBuilder,
    TopOptionsBuilder, UploadToContainerOptionsBuilder,
};
use bollard::Docker;
use bytes::Bytes;
use nexal_config::NexalConfig;
use tracing::{debug, info};

use crate::SandboxHandle;

/// Parse a human memory string like "512m", "1g", "2048k" into bytes.
fn parse_memory_bytes(s: &str) -> Option<i64> {
    let s = s.trim().to_lowercase();
    if let Some(rest) = s.strip_suffix('g') {
        rest.parse::<i64>().ok().map(|n| n * 1024 * 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.parse::<i64>().ok().map(|n| n * 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix('k') {
        rest.parse::<i64>().ok().map(|n| n * 1024)
    } else {
        s.parse::<i64>().ok()
    }
}

/// Parse a CPU string like "1.0", "0.5", "2" into nanocpus.
fn parse_nano_cpus(s: &str) -> Option<i64> {
    s.trim()
        .parse::<f64>()
        .ok()
        .map(|n| (n * 1_000_000_000.0) as i64)
}

/// Connect to the rootless Podman socket.
fn connect_podman() -> anyhow::Result<Docker> {
    // Podman rootless socket: $XDG_RUNTIME_DIR/podman/podman.sock
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    let socket_path = format!("{runtime_dir}/podman/podman.sock");
    Docker::connect_with_unix(&socket_path, 120, bollard::API_DEFAULT_VERSION)
        .context("connecting to Podman socket")
}

/// Create a tar archive containing a single file, suitable for
/// `upload_to_container`. The file is placed at `filename` inside the
/// archive with mode 0o755.
fn tar_single_file(filename: &str, content: &[u8]) -> anyhow::Result<Bytes> {
    let mut ar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path(filename)?;
    header.set_size(content.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    ar.append(&header, content)?;
    let data = ar.into_inner()?;
    Ok(Bytes::from(data))
}

/// Create, configure, and start the sandbox container via the Podman API.
///
/// Steps:
/// 1. Force-remove any stale container with the same name.
/// 2. Create with all sandbox options (userns, caps, resources, network).
/// 3. Upload the nexal-exec-server binary into the container.
/// 4. Start the container.
/// 5. Inspect to discover the host-mapped port for exec-server.
/// 6. Connect via WebSocket and build the EnvironmentManager.
pub async fn create_sandbox_container(config: &NexalConfig) -> anyhow::Result<SandboxHandle> {
    let docker = connect_podman()?;
    let name = format!("nexal-{}", crate::short_id());
    let image = config.sandbox_image.clone();
    let container_port = 9100u16;

    debug!(container = %name, image = %image, "creating sandbox container via API");

    // Step 1: remove stale container (ignore errors).
    let _ = docker
        .remove_container(
            &name,
            Some(RemoveContainerOptionsBuilder::default().force(true).build()),
        )
        .await;

    // Step 2: create container.
    let port_key = format!("{container_port}/tcp");
    let exposed_ports = vec![port_key.clone()];

    let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
    port_bindings.insert(
        port_key.clone(),
        Some(vec![PortBinding {
            host_ip: None,
            host_port: None, // let Podman pick a random host port
        }]),
    );

    let host_config = HostConfig {
        userns_mode: Some("keep-id".to_string()),
        security_opt: Some(vec!["no-new-privileges".to_string()]),
        cap_drop: Some(vec!["ALL".to_string()]),
        pids_limit: Some(config.sandbox_pids_limit as i64),
        memory: parse_memory_bytes(&config.sandbox_memory),
        nano_cpus: parse_nano_cpus(&config.sandbox_cpus),
        network_mode: Some(if config.sandbox_network {
            "pasta".to_string()
        } else {
            "none".to_string()
        }),
        dns: Some(vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()]),
        binds: Some(vec![format!(
            "{}:/workspace",
            config.workspace.display()
        )]),
        port_bindings: Some(port_bindings),
        runtime: config.sandbox_runtime.clone(),
        ..Default::default()
    };

    let container_config = ContainerCreateBody {
        image: Some(image.clone()),
        cmd: Some(vec![
            "/usr/local/bin/nexal-exec-server".to_string(),
            "--listen".to_string(),
            format!("ws://0.0.0.0:{container_port}"),
        ]),
        env: Some(vec!["HOME=/workspace".to_string()]),
        working_dir: Some("/workspace".to_string()),
        exposed_ports: Some(exposed_ports),
        host_config: Some(host_config),
        ..Default::default()
    };

    docker
        .create_container(
            Some(
                bollard::query_parameters::CreateContainerOptionsBuilder::default()
                    .name(&name)
                    .build(),
            ),
            container_config,
        )
        .await
        .context("podman create")?;
    debug!(container = %name, "container created");

    // Step 3: inject exec-server binary.
    let exec_server_bin = crate::find_exec_server_binary()?;
    debug!(binary = %exec_server_bin.display(), "uploading nexal-exec-server");
    let bin_content = tokio::fs::read(&exec_server_bin)
        .await
        .context("reading nexal-exec-server binary")?;
    let tar_body = tar_single_file("nexal-exec-server", &bin_content)?;
    docker
        .upload_to_container(
            &name,
            Some(
                UploadToContainerOptionsBuilder::default()
                    .path("/usr/local/bin")
                    .build(),
            ),
            bollard::body_full(tar_body),
        )
        .await
        .context("uploading exec-server binary")?;
    debug!(container = %name, "binary uploaded");

    // Step 4: start.
    docker
        .start_container(&name, None::<bollard::query_parameters::StartContainerOptions>)
        .await
        .context("podman start")?;
    debug!(container = %name, "container started");

    // Step 5: discover host-mapped port via inspect.
    let host_url = {
        let mut url = String::new();
        for attempt in 1..=20 {
            let info = docker
                .inspect_container(
                    &name,
                    Some(InspectContainerOptionsBuilder::default().build()),
                )
                .await;
            if let Ok(info) = info {
                if let Some(ports) = info
                    .network_settings
                    .as_ref()
                    .and_then(|ns| ns.ports.as_ref())
                    .and_then(|p| p.get(&port_key))
                    .and_then(|v| v.as_ref())
                {
                    if let Some(binding) = ports.first() {
                        if let Some(host_port) = &binding.host_port {
                            let host_ip = binding
                                .host_ip
                                .as_deref()
                                .unwrap_or("127.0.0.1");
                            url = format!("ws://{host_ip}:{host_port}");
                            break;
                        }
                    }
                }
            }
            if attempt < 20 {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
        if url.is_empty() {
            anyhow::bail!(
                "failed to discover mapped port for exec-server container {name}"
            );
        }
        url
    };
    debug!(container = %name, host_url = %host_url, "discovered port mapping");

    // Step 6: WebSocket connect to exec-server with retry.
    let client = {
        let mut client = None;
        for attempt in 1..=20 {
            match nexal_exec_server::ExecServerClient::connect_websocket(
                nexal_exec_server::RemoteExecServerConnectArgs::new(
                    host_url.clone(),
                    "nexal-sandbox".to_string(),
                ),
            )
            .await
            {
                Ok(c) => {
                    client = Some(c);
                    break;
                }
                Err(e) if attempt < 20 => {
                    debug!(container = %name, attempt, "WebSocket connect retry: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => {
                    anyhow::bail!("connect to exec-server WebSocket: {e}");
                }
            }
        }
        client.ok_or_else(|| anyhow::anyhow!("exec-server WebSocket connect exhausted retries"))?
    };
    debug!(container = %name, "WebSocket connected");

    let init_resp = client.init_response().clone();
    let env_info = nexal_exec_server::RemoteEnvInfo {
        default_shell: init_resp.default_shell,
        cwd: init_resp.cwd,
    };
    info!(
        container = %name,
        shell = ?env_info.default_shell,
        cwd = ?env_info.cwd,
        "exec-server ready"
    );
    let env = nexal_exec_server::Environment::create_from_client_with_env_info(client, env_info);
    let env_manager = Arc::new(nexal_exec_server::EnvironmentManager::with_environment(env));

    info!(container = %name, url = %host_url, "sandbox container ready");
    Ok(SandboxHandle {
        name,
        child: None,
        environment_manager: Some(env_manager),
    })
}

/// Check whether the container has processes beyond the exec-server itself.
pub async fn container_has_active_tasks(name: &str) -> bool {
    let Ok(docker) = connect_podman() else {
        return false;
    };
    let top = docker
        .top_processes(
            name,
            Some(TopOptionsBuilder::default().ps_args("eo comm").build()),
        )
        .await;
    let Ok(top) = top else { return false };
    let Some(processes) = top.processes else {
        return false;
    };
    processes
        .iter()
        .filter_map(|row| row.first())
        .filter(|cmd| {
            let cmd = cmd.trim();
            !cmd.is_empty() && cmd != "COMMAND" && cmd != "sleep" && cmd != "nexal-exec-s"
        })
        .count()
        > 0
}

/// Remove the container. If active tasks are running, prompt the user first.
pub async fn cleanup_sandbox_container(name: &str) {
    if container_has_active_tasks(name).await {
        info!("container {name} still has running tasks, waiting for user input");
        eprint!("Container has running tasks. Kill? [Y/n] ");

        let choice = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            let _ = std::io::stdin().read_line(&mut buf);
            buf.trim().to_lowercase()
        })
        .await
        .unwrap_or_default();

        if choice.starts_with('n') {
            info!("leaving container {name} running (podman rm -f {name} to remove)");
            return;
        }
    }

    let Ok(docker) = connect_podman() else {
        return;
    };
    let _ = docker
        .remove_container(
            name,
            Some(RemoveContainerOptionsBuilder::default().force(true).build()),
        )
        .await;
    info!("sandbox container removed: {name}");
}
