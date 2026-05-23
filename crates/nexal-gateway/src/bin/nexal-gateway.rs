use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use nexal_gateway::backend::{
    FlyBackend, FlyConfig, KubernetesBackend, KubernetesConfig, PodmanBackend, SharedBackend,
};
use nexal_gateway::config::GatewayConfig;
use nexal_gateway::pool::{self, WarmPool, WarmPoolConfig};
use nexal_gateway::proxy::{ProxyRegistry, serve_proxy};
use nexal_gateway::registry::SpawnDefaults;
use nexal_gateway::skills::SkillsService;
use nexal_gateway::{AgentRegistry, server::ServerConfig};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "nexal-gateway: host-side multiplexer for nexal-agent containers"
)]
struct Args {
    /// Path to the gateway TOML config. Defaults to ~/.nexal/gateway.toml.
    #[arg(long = "config", value_name = "PATH", env = "NEXAL_GATEWAY_CONFIG")]
    config: Option<PathBuf>,

    /// Override the WS listen address (e.g. `127.0.0.1:5500`).
    #[arg(long, env = "NEXAL_GATEWAY_LISTEN")]
    listen: Option<String>,

    /// Listen on a Unix domain socket instead of TCP.
    #[arg(long, env = "NEXAL_GATEWAY_UNIX")]
    unix: Option<PathBuf>,

    /// Override the proxy HTTP listen address (e.g. `0.0.0.0:5501`).
    #[arg(long = "proxy-listen", env = "NEXAL_GATEWAY_PROXY_LISTEN")]
    proxy_listen: Option<String>,

    /// Override the proxy URL prefix handed to agents.
    #[arg(
        long = "proxy-external-base",
        env = "NEXAL_GATEWAY_PROXY_EXTERNAL_BASE"
    )]
    proxy_external_base: Option<String>,

    /// Single-credential override for the embedded/dev path. Both must
    /// be set; merged into the gateway.toml `[[credentials]]` map.
    #[arg(long, env = "NEXAL_GATEWAY_ACCESS_KEY")]
    access_key: Option<String>,
    #[arg(long, env = "NEXAL_GATEWAY_SECRET_KEY")]
    secret_key: Option<String>,

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
    let mut credentials: HashMap<String, String> = cfg
        .credentials
        .iter()
        .filter(|c| !c.access_key.is_empty() && !c.secret_key.is_empty())
        .map(|c| (c.access_key.clone(), c.secret_key.clone()))
        .collect();
    if let (Some(ak), Some(sk)) = (args.access_key.clone(), args.secret_key.clone()) {
        credentials.insert(ak, sk);
    }
    if credentials.is_empty() {
        return Err("no credentials configured; add [[credentials]] (access_key/secret_key) to gateway.toml or set NEXAL_GATEWAY_ACCESS_KEY + NEXAL_GATEWAY_SECRET_KEY".into());
    }
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

    let backend: SharedBackend = match cfg.backend.kind.as_deref().unwrap_or("podman") {
        "podman" => Arc::new(PodmanBackend::new(
            cfg.backend.podman_bin.clone(),
            cfg.backend.runtime.clone(),
        )),
        "kubernetes" | "k8s" => {
            let agent_init_image = cfg
                .backend
                .agent_init_image
                .clone()
                .ok_or("kubernetes backend requires backend.agent_init_image")?;
            Arc::new(
                KubernetesBackend::new(KubernetesConfig {
                    namespace: cfg.backend.namespace.clone(),
                    kubeconfig: cfg.backend.kubeconfig.clone(),
                    agent_init_image,
                })
                .await?,
            )
        }
        "fly" => {
            let api_token = cfg
                .backend
                .fly_api_token
                .clone()
                .ok_or("fly backend requires backend.fly_api_token")?;
            let app = cfg
                .backend
                .fly_app
                .clone()
                .ok_or("fly backend requires backend.fly_app")?;
            Arc::new(FlyBackend::new(FlyConfig {
                api_token,
                app,
                region: cfg.backend.fly_region.clone(),
                api_base: cfg.backend.fly_api_base.clone(),
                agent_bin_path: cfg.backend.fly_agent_bin_path.clone(),
            })?)
        }
        other => return Err(format!("unknown backend kind: {other}").into()),
    };

    let defaults = SpawnDefaults {
        image,
        agent_bin,
        memory: cfg.defaults.memory.clone().or(Some("512m".into())),
        cpus: cfg.defaults.cpus.clone().or(Some("1.0".into())),
        pids_limit: cfg.defaults.pids_limit.or(Some(256)),
        network: cfg.defaults.network.unwrap_or(true),
        workspace_volume: cfg.defaults.workspace_volume.clone().or_else(|| {
            dirs::home_dir().map(|h| {
                h.join(".nexal")
                    .join("workspace")
                    .to_string_lossy()
                    .into_owned()
            })
        }),
        container_name_prefix: cfg
            .defaults
            .container_name_prefix
            .clone()
            .unwrap_or_else(|| "nexal-worker-".into()),
    };

    // Ensure workspace_volume directory exists.
    if let Some(vol) = &defaults.workspace_volume {
        std::fs::create_dir_all(vol)?;
    }

    // Skills service.
    let skills_dir = cfg.defaults.skills_dir.clone().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/root"))
            .join(".nexal")
            .join("skills")
    });
    let skills = Arc::new(SkillsService::new(skills_dir));

    // Warm pool (optional).
    let warm_pool = if cfg.pool.enabled.unwrap_or(false) && cfg.pool.size.unwrap_or(0) > 0 {
        let pool_image = cfg
            .pool
            .image
            .clone()
            .unwrap_or_else(|| defaults.image.clone());
        let pool = Arc::new(WarmPool::new(
            backend.clone(),
            WarmPoolConfig {
                size: cfg.pool.size.unwrap_or(0),
                image: pool_image,
            },
            defaults.container_name_prefix.clone(),
            defaults.agent_bin.clone(),
            defaults.memory.clone(),
            defaults.cpus.clone(),
            defaults.pids_limit,
            defaults.network,
        ));
        pool::start_replenish_loop(pool.clone());
        Some(pool)
    } else {
        None
    };

    let proxies = Arc::new(ProxyRegistry::new());
    let tcp_proxies = Arc::new(nexal_gateway::proxy::TcpProxyRegistry::new());
    let registry = Arc::new(AgentRegistry::new(
        backend,
        defaults,
        proxies.clone(),
        tcp_proxies,
        warm_pool,
        skills.clone(),
    ));

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
            unix: args.unix,
            credentials,
            nonce_cache: Arc::new(Mutex::new(HashMap::new())),
            proxy_external_base,
            skills,
        },
        registry,
    )
    .await?;
    Ok(())
}
