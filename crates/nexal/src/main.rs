mod podman;

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use clap::Subcommand;
use nexal_agent::{Agent, AgentPool};
use nexal_channel_core::DebounceConfig;
use nexal_config::NexalConfig;
#[cfg(feature = "tui")]
use nexal_arg0::Arg0DispatchPaths;
#[cfg(feature = "tui")]
use nexal_config_loader::LoaderOverrides;
use nexal_state::StateDb;
#[cfg(feature = "tui")]
use nexal_tui::Cli as TuiCli;
use tracing::info;

#[derive(Parser)]
#[command(name = "nexal", version, about = "Nexal AI agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Also listen on Telegram (requires TELEGRAM_BOT_TOKEN)
    #[arg(long)]
    telegram: bool,

    /// Also listen on Discord (requires DISCORD_BOT_TOKEN)
    #[arg(long)]
    discord: bool,

    /// Also listen on HTTP (port configurable via http_channel_port, default 3000)
    #[arg(long)]
    http: bool,

    /// Enable periodic heartbeat (interval configurable via heartbeat_interval_mins)
    #[arg(long)]
    heartbeat: bool,

    /// Enable cron scheduler (jobs stored in workspace/agents/cron_jobs.json)
    #[arg(long)]
    cron: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Run as a headless daemon (no TUI, channels only)
    Idle(IdleArgs),
}

#[derive(Parser)]
struct IdleArgs {
    /// Enable the Telegram channel (requires TELEGRAM_BOT_TOKEN)
    #[arg(long)]
    telegram: bool,

    /// Enable the Discord channel (requires DISCORD_BOT_TOKEN)
    #[arg(long)]
    discord: bool,

    /// Enable the HTTP channel (port configurable via http_channel_port)
    #[arg(long)]
    http: bool,

    /// Enable periodic heartbeat (interval configurable via heartbeat_interval_mins)
    #[arg(long)]
    heartbeat: bool,

    /// Enable cron scheduler (jobs stored in workspace/agents/cron_jobs.json)
    #[arg(long)]
    cron: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Idle(args)) => {
            let config = Arc::new(NexalConfig::from_env());
            init_tracing(&config.otel);
            info!("admins: {:?}", config.admins);
            {
                let tg = nexal_channel_telegram::config::TelegramChannelConfig::from_nexal_config(&config);
                info!("telegram_allow_from: {:?}", tg.allow_from);
                info!("telegram_allow_chats: {:?}", tg.allow_chats);
            }
            run_idle(args, config).await
        }
        None => {
            #[cfg(feature = "tui")]
            {
                // Default: TUI, optionally with channels running alongside
                run_tui(cli.telegram, cli.discord, cli.http, cli.heartbeat, cli.cron).await
            }
            #[cfg(not(feature = "tui"))]
            {
                anyhow::bail!("TUI not available. Use `nexal idle` or build with --features tui")
            }
        }
    }
}

/// Which channels should be enabled for this run.
#[derive(Debug, Clone, Copy, Default)]
struct ChannelFlags {
    telegram: bool,
    discord: bool,
    http: bool,
    heartbeat: bool,
    cron: bool,
}

impl ChannelFlags {
    fn any_enabled(&self) -> bool {
        self.telegram || self.discord || self.http || self.heartbeat || self.cron
    }

    fn enabled_names(&self) -> Vec<&'static str> {
        [
            ("telegram", self.telegram),
            ("discord", self.discord),
            ("http", self.http),
            ("heartbeat", self.heartbeat),
            ("cron", self.cron),
        ]
        .into_iter()
        .filter_map(|(name, on)| on.then_some(name))
        .collect()
    }
}

/// Install the channel listeners selected by `flags` onto `bot`.
fn install_channels(
    bot: &mut Agent,
    flags: ChannelFlags,
    config: &Arc<NexalConfig>,
    db: &Arc<StateDb>,
) {
    if flags.telegram {
        bot.add_channel(nexal_channel_telegram::TelegramChannel::new(Arc::clone(config)));
    }
    if flags.discord {
        bot.add_channel(nexal_channel_discord::DiscordChannel::new(Arc::clone(config)));
    }
    if flags.http {
        bot.add_channel(nexal_channel_http::HttpChannel::new(Arc::clone(config)));
    }
    if flags.heartbeat {
        bot.add_channel(nexal_channel_heartbeat::HeartbeatChannel::new(Arc::clone(config)));
    }
    if flags.cron {
        bot.add_channel(nexal_channel_cron::CronChannel::new(
            Arc::clone(config),
            Arc::clone(db),
        ));
    }
}

/// Open the state database, start its read-only proxy, and return both the
/// shared handle and the proxy's background task handle.
async fn open_state_db(
    config: &NexalConfig,
) -> anyhow::Result<(Arc<StateDb>, tokio::task::JoinHandle<()>)> {
    let db = Arc::new(
        StateDb::open(&config.database_url())
            .await
            .context("opening state db")?,
    );
    let proxy_handle =
        nexal_agent::db_proxy::start_db_proxy(&config.workspace, Arc::clone(&db)).await;
    Ok((db, proxy_handle))
}

/// Cleanup a sandbox container: kill its child process (if any) and remove it.
async fn shutdown_sandbox(mut handle: SandboxHandle) {
    if let Some(ref mut child) = handle.child {
        let _ = child.kill().await;
    }
    podman::cleanup_sandbox_container(&handle.name).await;
}

/// Build a fully-configured `AgentPool` for the given runtime context.
fn build_pool(
    config: Arc<NexalConfig>,
    environment_manager: Option<Arc<nexal_exec_server::EnvironmentManager>>,
    signal_server: Option<Arc<nexal_agent::StateSignalServer>>,
) -> Arc<AgentPool> {
    let mut pool = AgentPool::new(config);
    if let Some(env_mgr) = environment_manager {
        pool = pool.with_environment_manager(env_mgr);
    }
    if let Some(server) = signal_server {
        pool = pool.with_signal_server(server);
    }
    pool.into_shared()
}

/// Start the push-based state signal server, logging and returning `None` on
/// failure (the agent still runs, just without push state transitions).
async fn start_signal_server(
    agents_dir: &std::path::Path,
) -> Option<Arc<nexal_agent::StateSignalServer>> {
    match nexal_agent::StateSignalServer::start(agents_dir).await {
        Ok(server) => Some(Arc::new(server)),
        Err(e) => {
            tracing::warn!("failed to start state signal server: {e}");
            None
        }
    }
}

fn debounce_config_from(config: &NexalConfig) -> DebounceConfig {
    DebounceConfig {
        debounce_secs: config.debounce_secs,
        delay_secs: config.message_delay_secs,
        active_window_secs: config.active_window_secs,
    }
}

#[cfg(feature = "tui")]
async fn run_tui(enable_telegram: bool, enable_discord: bool, enable_http: bool, enable_heartbeat: bool, enable_cron: bool) -> anyhow::Result<()> {
    let config = Arc::new(NexalConfig::from_env());

    // Ensure workspace exists
    tokio::fs::create_dir_all(&config.workspace)
        .await
        .context("creating workspace dir")?;

    sync_skills(&config).await?;

    // Unified StateDb — same as idle/bot mode.
    // chatlog/toollog skills query this database inside the container.
    let (db, _db_proxy_handle) = open_state_db(&config).await?;

    // Sync TUI session events to StateDb for chatlog/toollog skills.
    let nexal_home = config.nexal_home.clone().unwrap_or_else(|| {
        std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
            .join(".nexal")
    });
    let _sync_handle = nexal_agent::db_sync::start_sync(Arc::clone(&db), &nexal_home);

    // Create a persistent Podman container for this session.
    // All exec commands run inside this container. API proxy sockets are
    // registered via exec-server so tokens stay on the host while the
    // container-side skill scripts talk to a local Unix socket.
    let sandbox = podman::create_sandbox_container(&config).await?;
    register_sandbox_proxies(&sandbox, &config).await;

    // Build TUI CLI with nexal defaults
    let mut tui_cli = TuiCli::parse_from(["nexal"]);
    // TUI always uses the host-side workspace path. The Podman sandbox
    // only affects exec commands (which get wrapped with `podman exec`).
    tui_cli.cwd = Some(config.workspace.clone());
    tui_cli
        .config_overrides
        .raw_overrides
        .push("project_doc_fallback_filenames=[\"SOUL.md\"]".to_string());

    // Inject providers from figment config into TUI's core config.
    // The headless agent path uses the same helper, so TUI and idle share
    // one source of truth for provider → override mapping.
    for (key, value) in nexal_agent::providers_to_cli_overrides_full(&config) {
        tui_cli
            .config_overrides
            .raw_overrides
            .push(format!("{key}={value}"));
    }

    // Start channel listeners alongside TUI if requested.
    let flags = ChannelFlags {
        telegram: enable_telegram
            || nexal_channel_telegram::config::TelegramChannelConfig::from_nexal_config(&config)
                .bot_token
                .is_some(),
        discord: enable_discord
            || nexal_channel_discord::config::DiscordChannelConfig::from_nexal_config(&config)
                .bot_token
                .is_some(),
        http: enable_http,
        heartbeat: enable_heartbeat,
        cron: enable_cron,
    };
    let bot_handle =
        maybe_start_channels(flags, Arc::clone(&config), Arc::clone(&db)).await?;

    // Run TUI (blocks until exit)
    let result = nexal_tui::run_main(
        tui_cli,
        Arg0DispatchPaths::default(),
        LoaderOverrides::default(),
    )
    .await;

    // Cleanup
    if let Some(handle) = bot_handle {
        handle.abort();
    }
    shutdown_sandbox(sandbox).await;

    result.map_err(|e| anyhow::anyhow!("TUI error: {e}"))?;
    Ok(())
}

#[cfg(feature = "tui")]
/// Start channel bot in the background if any channels are requested.
/// Returns a JoinHandle that can be aborted on TUI exit.
async fn maybe_start_channels(
    flags: ChannelFlags,
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    if !flags.any_enabled() {
        return Ok(None);
    }

    // In TUI mode, redirect channel logs to a file so they don't
    // corrupt the terminal UI.
    init_tracing_to_file(&config.workspace);

    let signal_server = start_signal_server(&config.workspace.join("agents")).await;
    let pool = build_pool(Arc::clone(&config), None, signal_server);

    let mut bot = Agent::new(Arc::clone(&pool), debounce_config_from(&config));
    install_channels(&mut bot, flags, &config, &db);

    info!(
        "starting channels alongside TUI: {}",
        flags.enabled_names().join(", ")
    );

    let handle = tokio::spawn(async move {
        if let Err(e) = bot.run().await {
            tracing::error!("channel bot error: {e}");
        }
    });

    Ok(Some(handle))
}

/// Initialize tracing to a log file (for TUI mode, so logs don't corrupt the terminal).
#[cfg(feature = "tui")]
fn init_tracing_to_file(workspace_dir: &std::path::Path) {
    let log_dir = workspace_dir.join("agents").join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("channels.log"))
        .ok();
    if let Some(file) = file {
        tracing_subscriber::fmt()
            .with_writer(std::sync::Mutex::new(file))
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nexal=info,warn")),
            )
            .with_ansi(false)
            .try_init()
            .ok();
    }
}

/// Generate a short random ID (8 lowercase alphanumeric chars).
pub(crate) fn short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ std::process::id() as u64;
    let mut n = seed;
    let alphabet = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut id = String::with_capacity(8);
    for _ in 0..8 {
        id.push(alphabet[(n % 36) as usize] as char);
        n /= 36;
        n ^= n.wrapping_mul(6364136223846793005);
    }
    id
}

/// Register all configured API proxy sockets inside the sandbox container
/// via exec-server. The exec-server creates Unix sockets at
/// `/workspace/agents/proxy/<host>` inside the container, forwards incoming
/// requests to the real upstream API, and injects auth tokens that never
/// leave the host. No bind mounts, no host-side proxy process.
///
/// This is the single unified proxy startup path — both `run_tui` and
/// `run_idle` call it right after `create_sandbox_container`. Channels
/// inside the container POST to the sockets via skill scripts.
async fn register_sandbox_proxies(sandbox: &SandboxHandle, config: &NexalConfig) {
    let Some(env_mgr) = sandbox.environment_manager.as_ref() else {
        return;
    };
    let Ok(env) = env_mgr.current().await else {
        tracing::warn!("exec-server environment unavailable; skipping proxy registration");
        return;
    };
    let Some(client) = env.exec_server_client() else {
        tracing::warn!("no exec-server client on environment; skipping proxy registration");
        return;
    };

    use std::collections::HashMap;

    let tg_token = nexal_channel_telegram::config::TelegramChannelConfig::from_nexal_config(config).bot_token;
    let dc_token = nexal_channel_discord::config::DiscordChannelConfig::from_nexal_config(config).bot_token;

    if let Some(ref token) = tg_token {
        // Telegram Bot API uses the token in the URL path.
        let upstream = format!("https://api.telegram.org/bot{token}");
        let params = nexal_exec_server::ProxyRegisterParams {
            socket_path: "/workspace/agents/proxy/api.telegram.org".to_string(),
            upstream_url: upstream,
            headers: HashMap::new(),
        };
        match client.proxy_register(params).await {
            Ok(_) => info!("container proxy registered: api.telegram.org"),
            Err(e) => tracing::warn!("failed to register telegram proxy: {e}"),
        }
    }

    if let Some(ref token) = dc_token {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bot {token}"));
        let params = nexal_exec_server::ProxyRegisterParams {
            socket_path: "/workspace/agents/proxy/discord.com".to_string(),
            upstream_url: "https://discord.com".to_string(),
            headers,
        };
        match client.proxy_register(params).await {
            Ok(_) => info!("container proxy registered: discord.com"),
            Err(e) => tracing::warn!("failed to register discord proxy: {e}"),
        }
    }

    if let Some(ref key) = config.jina_api_key {
        // Jina Search API (s.jina.ai)
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {key}"));
        headers.insert("Accept".to_string(), "application/json".to_string());
        let params = nexal_exec_server::ProxyRegisterParams {
            socket_path: "/workspace/agents/proxy/s.jina.ai".to_string(),
            upstream_url: "https://s.jina.ai".to_string(),
            headers: headers.clone(),
        };
        match client.proxy_register(params).await {
            Ok(_) => info!("container proxy registered: s.jina.ai (search)"),
            Err(e) => tracing::warn!("failed to register jina search proxy: {e}"),
        }

        // Jina Reader API (r.jina.ai)
        let params = nexal_exec_server::ProxyRegisterParams {
            socket_path: "/workspace/agents/proxy/r.jina.ai".to_string(),
            upstream_url: "https://r.jina.ai".to_string(),
            headers,
        };
        match client.proxy_register(params).await {
            Ok(_) => info!("container proxy registered: r.jina.ai (reader)"),
            Err(e) => tracing::warn!("failed to register jina reader proxy: {e}"),
        }
    }
}


pub(crate) struct SandboxHandle {
    pub name: String,
    /// For krun: the running `podman run` child process (keeps the VM alive).
    pub child: Option<tokio::process::Child>,
    /// Pre-connected environment manager using exec-server transport.
    pub environment_manager: Option<Arc<nexal_exec_server::EnvironmentManager>>,
}

/// Embedded nexal-exec-server binary. Set `NEXAL_EXEC_SERVER_BIN` at compile time to embed.
/// When not set, falls back to searching the filesystem.
#[cfg(feature = "embedded-agent")]
static EMBEDDED_EXEC_SERVER: &[u8] = include_bytes!(env!("NEXAL_EXEC_SERVER_BIN"));

pub(crate) fn find_exec_server_binary() -> anyhow::Result<std::path::PathBuf> {
    // 1. Try embedded binary — extract to a cache directory once.
    #[cfg(feature = "embedded-agent")]
    {
        return extract_embedded_exec_server();
    }

    // 2. Filesystem search fallback (development builds without embedded agent).
    #[allow(unreachable_code)]
    find_exec_server_binary_on_disk()
}

#[cfg(feature = "embedded-agent")]
fn extract_embedded_exec_server() -> anyhow::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let cache_dir = dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("nexal");
    std::fs::create_dir_all(&cache_dir).context("create nexal cache dir")?;

    let bin_path = cache_dir.join("nexal-exec-server");

    // Only re-extract if size differs (cheap staleness check).
    let needs_extract = match std::fs::metadata(&bin_path) {
        Ok(meta) => meta.len() != EMBEDDED_EXEC_SERVER.len() as u64,
        Err(_) => true,
    };

    if needs_extract {
        std::fs::write(&bin_path, EMBEDDED_EXEC_SERVER)
            .context("extract embedded nexal-exec-server")?;
        std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755))
            .context("chmod nexal-exec-server")?;
    }

    Ok(bin_path)
}

fn find_exec_server_binary_on_disk() -> anyhow::Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("resolve current executable")?;
    let exe_dir = exe.parent().unwrap_or(std::path::Path::new("."));

    let candidates = [
        exe_dir.join("nexal-exec-server"),
        exe_dir.join("../release/nexal-exec-server"),
        exe_dir.join("../debug/nexal-exec-server"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.canonicalize().unwrap_or(candidate.clone()));
        }
    }

    anyhow::bail!(
        "nexal-exec-server binary not found. Either:\n\
         - Build with embedding: cargo build --release -p nexal-exec-server && \
           NEXAL_EXEC_SERVER_BIN=target/release/nexal-exec-server cargo build --features embedded-agent\n\
         - Or place nexal-exec-server next to the nexal binary\n\
         Searched: {}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

async fn sync_skills(config: &NexalConfig) -> anyhow::Result<()> {
    // Layout:
    //   agents/skills/          ← built-in, copied from source on startup
    //   agents/skills.override/ ← agent-created (read-write)
    // Both are inside workspace, so they're visible in the Podman container.
    // The agent can modify files in skills/ at runtime; they'll be overwritten
    // on next startup from the source.
    let agents_dir = config.workspace.join("agents");
    let _ = tokio::fs::create_dir_all(&agents_dir).await;
    let _ = tokio::fs::create_dir_all(agents_dir.join("skills.override")).await;
    // Ensure uv/pip cache dir exists so tools can install dependencies inside the container.
    let _ = tokio::fs::create_dir_all(config.workspace.join(".cache")).await;
    let skills_dst = agents_dir.join("skills");

    let candidates: Vec<std::path::PathBuf> = [
        config.skills_dir.clone(),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills")),
    ]
    .into_iter()
    .flatten()
    .collect();

    let skills_src = candidates.iter().find(|p| p.is_dir());
    let Some(skills_src) = skills_src else {
        return Ok(());
    };

    // Remove old symlink or stale directory, then copy fresh from source.
    if skills_dst.read_link().is_ok() {
        tokio::fs::remove_file(&skills_dst).await.ok();
    } else if skills_dst.is_dir() {
        tokio::fs::remove_dir_all(&skills_dst).await.ok();
    }

    let src = skills_src.canonicalize().unwrap_or(skills_src.clone());
    copy_dir_recursive(&src, &skills_dst).await?;

    Ok(())
}

/// Recursively copy a directory tree.
async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().await?.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

/// Initialize tracing to stderr, optionally with OTLP export.
fn init_tracing(otel: &nexal_config::OtelConfig) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nexal=info,rmcp=off,warn"));

    let fmt_layer = tracing_subscriber::fmt::layer();

    if let Some((otel_layer, endpoint)) = init_otel_layer(otel) {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .try_init()
            .ok();
        tracing::info!(endpoint = %endpoint, "OTLP tracing enabled");
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .ok();
    }
}

/// Build an OpenTelemetry tracing layer if OTLP is configured.
fn init_otel_layer<S>(
    otel: &nexal_config::OtelConfig,
) -> Option<(tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>, String)>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_otlp::WithHttpConfig;

    // Environment variable overrides config file.
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .or_else(|| otel.endpoint.clone())
        .filter(|e| !e.is_empty())?;

    let service_name = otel
        .service_name
        .clone()
        .or_else(|| std::env::var("OTEL_SERVICE_NAME").ok())
        .unwrap_or_else(|| "nexal".to_string());

    // Merge config headers with env var headers (env takes precedence).
    let mut headers = otel.headers.clone();
    if let Ok(env_headers) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
        for pair in env_headers.split(',') {
            if let Some((k, v)) = pair.split_once('=') {
                headers.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_http_client(reqwest::Client::new())
        .with_endpoint(&endpoint)
        .with_headers(headers)
        .build()
        .map_err(|e| eprintln!("failed to create OTLP exporter: {e}"))
        .ok()?;

    // Use the tokio-aware batch processor so HTTP requests run on the
    // tokio runtime instead of a bare OS thread (which has no reactor).
    let batch_processor =
        opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(
            exporter,
            opentelemetry_sdk::runtime::Tokio,
        )
        .build();

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_span_processor(batch_processor)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(service_name.clone())
                .build(),
        )
        .build();

    let tracer = provider.tracer(service_name);

    // Store provider in a global so it lives for the entire process.
    // Dropping it would shut down the batch processor.
    static PROVIDER: std::sync::OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> =
        std::sync::OnceLock::new();
    let _ = PROVIDER.set(provider);

    Some((tracing_opentelemetry::layer().with_tracer(tracer), endpoint))
}


async fn run_idle(args: IdleArgs, config: Arc<NexalConfig>) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&config.workspace)
        .await
        .context("creating workspace dir")?;

    sync_skills(&config).await?;

    // Open state database (for cron jobs, chatlog, etc.)
    let (db, _db_proxy_handle) = open_state_db(&config).await?;

    // Create Podman sandbox container.
    let sandbox = podman::create_sandbox_container(&config).await?;
    register_sandbox_proxies(&sandbox, &config).await;

    // Start the state signal socket for push-based BUSY→IDLE transitions.
    // Tool scripts (telegram_send, no_response, etc.) connect to this socket
    // to signal they have completed a response action.
    let signal_server = start_signal_server(&config.workspace.join("agents")).await;

    let pool = build_pool(
        Arc::clone(&config),
        sandbox.environment_manager.clone(),
        signal_server,
    );

    let mut bot = Agent::new(Arc::clone(&pool), debounce_config_from(&config));

    // If any flag is explicit, only start flagged channels.
    // If no flags, auto-detect from configured tokens.
    let explicit = args.telegram || args.discord || args.http || args.heartbeat || args.cron;
    let flags = ChannelFlags {
        telegram: if explicit {
            args.telegram
        } else {
            nexal_channel_telegram::config::TelegramChannelConfig::from_nexal_config(&config)
                .bot_token
                .is_some()
        },
        discord: if explicit {
            args.discord
        } else {
            nexal_channel_discord::config::DiscordChannelConfig::from_nexal_config(&config)
                .bot_token
                .is_some()
        },
        http: args.http,
        heartbeat: args.heartbeat,
        cron: args.cron,
    };
    install_channels(&mut bot, flags, &config, &db);

    if !flags.any_enabled() {
        anyhow::bail!(
            "No channel configured. Set TELEGRAM_BOT_TOKEN, DISCORD_BOT_TOKEN, or use --http, --heartbeat, or --cron."
        );
    }

    let result = tokio::select! {
        result = bot.run() => result,
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down");
            Ok(())
        }
    };

    shutdown_sandbox(sandbox).await;

    result
}
