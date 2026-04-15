use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use nexal_gateway::backend::PodmanBackend;
use nexal_gateway::config::GatewayConfig;
use nexal_gateway::proxy::{serve_proxy, ProxyRegistry};
use nexal_gateway::registry::SpawnDefaults;
use nexal_gateway::{server::ServerConfig, AgentRegistry};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(version, about = "nexal-gateway: host-side multiplexer for nexal-agent containers")]
struct Args {
    /// Path to the gateway TOML config. Defaults to ~/.nexal/gateway.toml.
    #[arg(long = "config", value_name = "PATH", env = "NEXAL_GATEWAY_CONFIG")]
    config: Option<PathBuf>,

    /// Override the WS listen address (e.g. `127.0.0.1:5500`).
    #[arg(long, env = "NEXAL_GATEWAY_LISTEN")]
    listen: Option<String>,

    /// Override the proxy HTTP listen address (e.g. `0.0.0.0:5501`).
    #[arg(long = "proxy-listen", env = "NEXAL_GATEWAY_PROXY_LISTEN")]
    proxy_listen: Option<String>,

    /// Override the proxy URL prefix handed to agents.
    #[arg(long = "proxy-external-base", env = "NEXAL_GATEWAY_PROXY_EXTERNAL_BASE")]
    proxy_external_base: Option<String>,

    /// Override the shared auth token. Required if not set in config.
    #[arg(long, env = "NEXAL_GATEWAY_TOKEN")]
    token: Option<String>,

    /// Override the in-container nexal-agent binary path.
    #[arg(long = "agent-bin", env = "NEXAL_AGENT_BIN")]
    agent_bin: Option<PathBuf>,

    /// Override the default sandbox image.
    #[arg(long, env = "NEXAL_GATEWAY_IMAGE")]
    image: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NEXAL_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let cfg_path = args
        .config
        .clone()
        .or_else(GatewayConfig::default_path)
        .ok_or("could not resolve gateway config path (no --config and no $HOME)")?;
    let cfg = GatewayConfig::load(&cfg_path).await?;

    let listen = args
        .listen
        .or(cfg.listen.clone())
        .unwrap_or_else(|| "127.0.0.1:5500".to_string());
    let token = args.token.or(cfg.token.clone()).ok_or(
        "no shared token configured; pass --token or set token in gateway.toml / NEXAL_GATEWAY_TOKEN",
    )?;
    let agent_bin = args
        .agent_bin
        .or(cfg.defaults.agent_bin.clone())
        .ok_or("no agent_bin configured; pass --agent-bin or set defaults.agent_bin")?;
    let image = args
        .image
        .or(cfg.defaults.image.clone())
        .unwrap_or_else(|| "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13".to_string());
    let proxy_listen = args
        .proxy_listen
        .or(cfg.proxy.listen.clone())
        .unwrap_or_else(|| "0.0.0.0:5501".to_string());
    let proxy_external_base = args
        .proxy_external_base
        .or(cfg.proxy.external_base.clone())
        .unwrap_or_else(|| "http://host.containers.internal:5501".to_string());

    let backend = match cfg.backend.kind.as_deref().unwrap_or("podman") {
        "podman" => Arc::new(PodmanBackend::new(
            cfg.backend.podman_bin.clone(),
            cfg.backend.runtime.clone(),
        )) as Arc<_>,
        other => return Err(format!("unknown backend kind: {other}").into()),
    };

    let defaults = SpawnDefaults {
        image,
        agent_bin,
        workspace: cfg.defaults.workspace.clone(),
        memory: cfg.defaults.memory.clone().or(Some("512m".into())),
        cpus: cfg.defaults.cpus.clone().or(Some("1.0".into())),
        pids_limit: cfg.defaults.pids_limit.or(Some(256)),
        network: cfg.defaults.network.unwrap_or(true),
        container_name_prefix: cfg
            .defaults
            .container_name_prefix
            .clone()
            .unwrap_or_else(|| "nexal-worker-".into()),
    };

    let proxies = Arc::new(ProxyRegistry::new());
    let registry = Arc::new(AgentRegistry::new(backend, defaults, proxies.clone()));

    // Graceful shutdown — detach (not destroy) all agents on Ctrl-C.
    let registry_for_shutdown = registry.clone();
    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            tracing::info!("ctrl-c received, detaching agents (containers stay alive)");
            registry_for_shutdown.detach_all().await;
            std::process::exit(0);
        }
    });

    // Spawn the proxy server alongside the WS server. Both run forever
    // until the process exits.
    let proxy_listen_clone = proxy_listen.clone();
    tokio::spawn(async move {
        if let Err(err) = serve_proxy(proxy_listen_clone, proxies).await {
            tracing::error!("proxy server failed: {err}");
        }
    });

    nexal_gateway::serve(
        ServerConfig {
            listen,
            token,
            proxy_external_base,
        },
        registry,
    )
    .await?;
    Ok(())
}
