use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use clap::Subcommand;
use nexal_agent::{AgentPool, Bot};
use nexal_arg0::Arg0DispatchPaths;
use nexal_channel_core::DebounceConfig;
use nexal_config::NexalConfig;
use nexal_config_loader::LoaderOverrides;
use nexal_state::StateDb;
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
            info!("telegram_allow_from: {:?}", config.telegram_allow_from);
            info!("telegram_allow_chats: {:?}", config.telegram_allow_chats);
            run_idle(args, config).await
        }
        None => {
            // Default: TUI, optionally with channels running alongside
            run_tui(cli.telegram, cli.discord, cli.http).await
        }
    }
}

async fn run_tui(enable_telegram: bool, enable_discord: bool, enable_http: bool) -> anyhow::Result<()> {
    let config = Arc::new(NexalConfig::from_env());

    // Ensure workspace exists
    tokio::fs::create_dir_all(&config.workspace)
        .await
        .context("creating workspace dir")?;

    sync_skills(&config).await?;

    // Unified StateDb — same as idle/bot mode.
    // chatlog/toollog skills query this database inside the container.
    let db_path = config.workspace.join("agents").join("nexal.db");
    let _ = tokio::fs::create_dir_all(db_path.parent().unwrap()).await;
    let db = Arc::new(
        StateDb::open(&db_path)
            .await
            .context("opening state db")?,
    );

    // Sync TUI session events to StateDb for chatlog/toollog skills.
    let nexal_home = config
        .nexal_home
        .clone()
        .unwrap_or_else(|| {
            std::env::var("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
                .join(".nexal")
        });
    let _sync_handle = nexal_agent::db_sync::start_sync(Arc::clone(&db), &nexal_home);

    // Start token proxies (Unix sockets for Telegram/Discord API access).
    // Tokens stay on the host; container connects via socket.
    let _proxy_handles = nexal_agent::proxy::start_proxies(
        &config.workspace,
        config.telegram_bot_token.as_deref(),
        config.discord_bot_token.as_deref(),
    )
    .await;

    // Create a persistent Podman container for this session.
    // All exec commands run inside this container.
    // Set NEXAL_SANDBOX=none to disable (not recommended).
    let sandbox = if !is_sandbox_disabled(&config) {
        Some(create_sandbox_container(&config).await?)
    } else {
        None
    };

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
    for (name, provider) in &config.providers {
        // name is required by core
        let display = provider.name.as_deref().unwrap_or(name.as_str());
        tui_cli.config_overrides.raw_overrides
            .push(format!("model_providers.{name}.name=\"{display}\""));
        if let Some(ref url) = provider.base_url {
            tui_cli.config_overrides.raw_overrides
                .push(format!("model_providers.{name}.base_url=\"{url}\""));
        }
        if let Some(ref key) = provider.env_key {
            tui_cli.config_overrides.raw_overrides
                .push(format!("model_providers.{name}.env_key=\"{key}\""));
        }
        if let Some(ref api) = provider.wire_api {
            tui_cli.config_overrides.raw_overrides
                .push(format!("model_providers.{name}.wire_api=\"{api}\""));
        }
        if provider.thinking_mode {
            tui_cli.config_overrides.raw_overrides
                .push(format!("model_providers.{name}.thinking_mode=true"));
        }
    }

    // Auto-select the first custom provider if any are configured.
    if let Some(provider_id) = config.providers.keys().next() {
        tui_cli.config_overrides.raw_overrides
            .push(format!("model_provider=\"{provider_id}\""));
    }

    // Start channel listeners alongside TUI if requested.
    let bot_handle = maybe_start_channels(
        enable_telegram,
        enable_discord,
        enable_http,
        Arc::clone(&config),
    )
    .await?;

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
    if let Some(mut handle) = sandbox {
        if let Some(ref mut child) = handle.child {
            let _ = child.kill().await;
        }
        cleanup_sandbox_container(&handle.name).await;
    }

    result.map_err(|e| anyhow::anyhow!("TUI error: {e}"))?;
    Ok(())
}

/// Start channel bot in the background if any channels are requested.
/// Returns a JoinHandle that can be aborted on TUI exit.
async fn maybe_start_channels(
    enable_telegram: bool,
    enable_discord: bool,
    enable_http: bool,
    config: Arc<NexalConfig>,
) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    let run_telegram = enable_telegram || config.telegram_bot_token.is_some();
    let run_discord = enable_discord || config.discord_bot_token.is_some();
    let run_http = enable_http;

    if !run_telegram && !run_discord && !run_http {
        return Ok(None);
    }

    // In TUI mode, redirect channel logs to a file so they don't
    // corrupt the terminal UI.
    init_tracing_to_file(&config.workspace);

    let pool = AgentPool::new(Arc::clone(&config));
    let debounce_config = DebounceConfig {
        debounce_secs: config.debounce_secs,
        delay_secs: config.message_delay_secs,
        active_window_secs: config.active_window_secs,
    };

    let mut bot = Bot::new(
        Arc::clone(&pool),
        debounce_config,
    );

    if run_telegram {
        bot.add_channel(nexal_channel_telegram::TelegramChannel::new(
            Arc::clone(&config),
        ));
    }
    if run_discord {
        bot.add_channel(nexal_channel_discord::DiscordChannel::new(
            Arc::clone(&config),
        ));
    }
    if run_http {
        bot.add_channel(nexal_channel_http::HttpChannel::new(
            Arc::clone(&config),
        ));
    }

    let mut channels: Vec<&str> = Vec::new();
    if run_telegram { channels.push("telegram"); }
    if run_discord { channels.push("discord"); }
    if run_http { channels.push("http"); }
    info!("starting channels alongside TUI: {}", channels.join(", "));

    let handle = tokio::spawn(async move {
        if let Err(e) = bot.run().await {
            tracing::error!("channel bot error: {e}");
        }
    });

    Ok(Some(handle))
}

/// Generate a short random ID (8 lowercase alphanumeric chars).
fn short_id() -> String {
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

/// Podman sandbox is enabled by default. Disable via config: sandbox = "none"
fn is_sandbox_disabled(config: &NexalConfig) -> bool {
    config.sandbox_backend() == nexal_config::SandboxBackend::None
}

struct SandboxHandle {
    name: String,
    /// For krun: the running `podman run` child process (keeps the VM alive).
    child: Option<tokio::process::Child>,
    /// For krun: pre-connected environment manager using child-process transport.
    environment_manager: Option<Arc<nexal_exec_server::EnvironmentManager>>,
}

fn sandbox_common_args(config: &NexalConfig, name: &str) -> Vec<String> {
    vec![
        "--name".to_string(),
        name.to_string(),
        "--userns=keep-id".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--cap-drop=ALL".to_string(),
        format!("--pids-limit={}", config.sandbox_pids_limit),
        format!("--memory={}", config.sandbox_memory),
        format!("--cpus={}", config.sandbox_cpus),
        if config.sandbox_network {
            // Use pasta network (rootless podman default). Container ports
            // are accessible on the host's loopback via pasta port forwarding.
            "--network=pasta".to_string()
        } else {
            "--network=none".to_string()
        },
        "--dns=1.1.1.1".to_string(),
        "--dns=8.8.8.8".to_string(),
        // Set HOME to /workspace so tools like uv/pip write caches there
        // instead of the non-existent host home directory.
        "-e".to_string(),
        "HOME=/workspace".to_string(),
        "-v".to_string(),
        format!("{}:/workspace", config.workspace.display()),
        "-w".to_string(),
        "/workspace".to_string(),
    ]
}

/// Embedded nexal-exec-server binary. Set `NEXAL_EXEC_SERVER_BIN` at compile time to embed.
/// When not set, falls back to searching the filesystem.
#[cfg(feature = "embedded-agent")]
static EMBEDDED_EXEC_SERVER: &[u8] = include_bytes!(env!("NEXAL_EXEC_SERVER_BIN"));

fn find_exec_server_binary() -> anyhow::Result<std::path::PathBuf> {
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

async fn create_sandbox_container(config: &NexalConfig) -> anyhow::Result<SandboxHandle> {
    use tracing::debug;

    let name = format!("nexal-{}", short_id());
    let image = std::env::var("SANDBOX_IMAGE")
        .unwrap_or_else(|_| config.sandbox_image.clone());

    debug!(container = %name, image = %image, "creating sandbox container");

    let _ = tokio::process::Command::new("podman")
        .args(["rm", "-f", &name])
        .output()
        .await;

    let exec_server_bin = find_exec_server_binary()?;
    debug!(binary = %exec_server_bin.display(), "resolved nexal-exec-server binary");

    // Step 1: podman create
    let mut create_args = vec!["create".to_string()];
    create_args.extend(sandbox_common_args(config, &name));
    if let Some(ref runtime) = config.sandbox_runtime {
        create_args.push("--runtime".to_string());
        create_args.push(runtime.clone());
    }
    // Use a fixed container port and let podman assign a random host port.
    let container_port = 9100;
    create_args.push("-p".to_string());
    create_args.push(format!("{container_port}"));
    create_args.push(image.clone());
    create_args.push("/usr/local/bin/nexal-exec-server".to_string());
    create_args.push("--listen".to_string());
    create_args.push(format!("ws://0.0.0.0:{container_port}"));

    debug!(container = %name, "podman create");
    let output = tokio::process::Command::new("podman")
        .args(&create_args)
        .output()
        .await
        .context("podman create")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman create failed: {stderr}");
    }
    debug!(container = %name, "podman create succeeded");

    // Step 2: podman cp — inject exec-server binary before starting
    let container_dest = format!("{name}:/usr/local/bin/nexal-exec-server");
    debug!(
        src = %exec_server_bin.display(),
        dest = %container_dest,
        "podman cp nexal-exec-server into container"
    );
    let output = tokio::process::Command::new("podman")
        .args([
            "cp",
            &exec_server_bin.to_string_lossy(),
            &container_dest,
        ])
        .output()
        .await
        .context("podman cp nexal-exec-server")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman cp failed: {stderr}");
    }
    debug!(container = %name, "podman cp succeeded");

    // Step 3: podman start (detached) then query mapped port
    debug!(container = %name, "podman start");
    let start_output = tokio::process::Command::new("podman")
        .args(["start", &name])
        .output()
        .await
        .context("podman start")?;
    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        anyhow::bail!("podman start failed: {stderr}");
    }

    // Query the host-side mapped port via `podman port`.
    // Retry a few times — the container needs a moment to start.
    let mut host_url = String::new();
    for attempt in 1..=20 {
        let port_output = tokio::process::Command::new("podman")
            .args(["port", &name, &format!("{container_port}/tcp")])
            .output()
            .await
            .context("podman port")?;
        if port_output.status.success() {
            let mapping = String::from_utf8_lossy(&port_output.stdout).trim().to_string();
            // Output is like "127.0.0.1:43210"
            if !mapping.is_empty() {
                host_url = format!("ws://{mapping}");
                break;
            }
        }
        if attempt < 20 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    if host_url.is_empty() {
        anyhow::bail!("failed to discover mapped port for exec-server container {name}");
    }
    debug!(container = %name, host_url = %host_url, "discovered exec-server port mapping");

    // Connect via WebSocket with retry.
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
                debug!(container = %name, attempt, "WebSocket connect failed, retrying: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                anyhow::bail!("connect to nexal-exec-server WebSocket inside container: {e}");
            }
        }
    }
    let client = client.unwrap();
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
        "exec-server environment info"
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

/// Check if the sandbox container has active processes beyond `sleep infinity`.
async fn container_has_active_tasks(name: &str) -> bool {
    let output = tokio::process::Command::new("podman")
        .args(["top", name, "-eo", "comm"])
        .output()
        .await;
    let Ok(output) = output else { return false };
    if !output.status.success() { return false; }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line is a process name. Filter out the idle `sleep` and the header.
    stdout.lines()
        .filter(|line| {
            let cmd = line.trim();
            !cmd.is_empty() && cmd != "COMMAND" && cmd != "sleep"
        })
        .count() > 0
}

/// Cleanup sandbox container on exit.
/// If there are active tasks running inside, prompt the user before killing.
async fn cleanup_sandbox_container(name: &str) {
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

    let _ = tokio::process::Command::new("podman")
        .args(["rm", "-f", name])
        .output()
        .await;
    info!("sandbox container removed: {name}");
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

/// Initialize tracing to a log file (for TUI mode, so logs don't corrupt the terminal).
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

async fn run_idle(args: IdleArgs, config: Arc<NexalConfig>) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&config.workspace)
        .await
        .context("creating workspace dir")?;

    sync_skills(&config).await?;

    // Create Podman sandbox container for idle mode (same as TUI).
    let sandbox = if !is_sandbox_disabled(&config) {
        Some(create_sandbox_container(&config).await?)
    } else {
        None
    };

    // Start token proxies
    let _proxy_handles = nexal_agent::proxy::start_proxies(
        &config.workspace,
        config.telegram_bot_token.as_deref(),
        config.discord_bot_token.as_deref(),
    )
    .await;

    let mut pool = AgentPool::new(Arc::clone(&config));
    if let Some(ref handle) = sandbox {
        if let Some(ref env_mgr) = handle.environment_manager {
            pool = pool.with_environment_manager(Arc::clone(env_mgr));
        }
    }
    let debounce_config = DebounceConfig {
        debounce_secs: config.debounce_secs,
        delay_secs: config.message_delay_secs,
        active_window_secs: config.active_window_secs,
    };

    let mut bot = Bot::new(
        Arc::clone(&pool),
        debounce_config,
    );

    // If any flag is explicit, only start flagged channels.
    // If no flags, auto-detect from configured tokens.
    let explicit = args.telegram || args.discord || args.http;
    let run_telegram = if explicit { args.telegram } else { config.telegram_bot_token.is_some() };
    let run_discord = if explicit { args.discord } else { config.discord_bot_token.is_some() };
    let run_http = args.http;

    if run_telegram {
        bot.add_channel(nexal_channel_telegram::TelegramChannel::new(
            Arc::clone(&config),
        ));
    }
    if run_discord {
        bot.add_channel(nexal_channel_discord::DiscordChannel::new(
            Arc::clone(&config),
        ));
    }
    if run_http {
        bot.add_channel(nexal_channel_http::HttpChannel::new(
            Arc::clone(&config),
        ));
    }

    if !run_telegram && !run_discord && !run_http {
        anyhow::bail!(
            "No channel configured. Set TELEGRAM_BOT_TOKEN, DISCORD_BOT_TOKEN, or use --http."
        );
    }

    let result = tokio::select! {
        result = bot.run() => result,
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down");
            Ok(())
        }
    };

    // Cleanup sandbox container
    if let Some(mut handle) = sandbox {
        if let Some(ref mut child) = handle.child {
            let _ = child.kill().await;
        }
        cleanup_sandbox_container(&handle.name).await;
    }

    result
}
