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
}

#[derive(Subcommand)]
enum Command {
    /// Run as a background daemon serving Telegram and/or Discord
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Idle(args)) => {
            init_tracing();
            let config = Arc::new(NexalConfig::from_env());
            run_idle(args, config).await
        }
        None => {
            // Default: launch interactive TUI
            run_tui().await
        }
    }
}

async fn run_tui() -> anyhow::Result<()> {
    let config = NexalConfig::from_env();

    // Ensure workspace exists
    tokio::fs::create_dir_all(&config.workspace_dir)
        .await
        .context("creating workspace dir")?;

    // Sync skill directories into workspace
    sync_skills(&config).await?;

    // Build TUI CLI with nexal defaults
    let mut tui_cli = TuiCli::parse_from(["nexal"]);
    tui_cli.cwd = Some(config.workspace_dir.clone());

    // Tell nexal to also look for SOUL.md as project-level instructions
    tui_cli
        .config_overrides
        .raw_overrides
        .push("project_doc_fallback_filenames=[\"SOUL.md\"]".to_string());

    nexal_tui::run_main(
        tui_cli,
        Arg0DispatchPaths::default(),
        LoaderOverrides::default(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("TUI error: {e}"))?;

    Ok(())
}

/// Ensure skill directories are visible in the workspace.
async fn sync_skills(config: &NexalConfig) -> anyhow::Result<()> {
    let skills_dst = config.workspace_dir.join("skills");

    // If skills already exist in workspace (real dir, not stale symlink), done.
    if skills_dst.is_dir() && skills_dst.read_link().is_err() {
        return Ok(());
    }

    let candidates: Vec<std::path::PathBuf> = [
        config.skills_dir.clone(),
        // Dev layout: nexal-rs/nexal/../../../nexal/skills
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../nexal/skills")),
    ]
    .into_iter()
    .flatten()
    .collect();

    let skills_src = candidates.iter().find(|p| p.is_dir());
    let Some(skills_src) = skills_src else {
        return Ok(());
    };

    // Remove stale symlink if present
    if skills_dst.read_link().is_ok() {
        tokio::fs::remove_file(&skills_dst).await.ok();
    }

    let src = skills_src.canonicalize().unwrap_or(skills_src.clone());
    tokio::fs::symlink(&src, &skills_dst).await.ok();

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nexal=info,warn")),
        )
        .init();
}

async fn run_idle(args: IdleArgs, config: Arc<NexalConfig>) -> anyhow::Result<()> {
    // Ensure workspace exists
    tokio::fs::create_dir_all(&config.workspace_dir)
        .await
        .context("creating workspace dir")?;

    sync_skills(&config).await?;

    let db_path = config.workspace_dir.join("agents").join("nexal.db");
    tokio::fs::create_dir_all(db_path.parent().unwrap()).await?;
    let db = Arc::new(
        StateDb::open(&db_path)
            .await
            .context("opening state db")?,
    );

    info!("state db: {}", db_path.display());

    let pool = AgentPool::new(Arc::clone(&config));

    let debounce_config = DebounceConfig {
        debounce_secs: config.debounce_secs,
        delay_secs: config.message_delay_secs,
        active_window_secs: config.active_window_secs,
    };

    let mut bot = Bot::new(
        Arc::clone(&pool),
        Arc::clone(&config),
        Arc::clone(&db),
        debounce_config,
    );

    // Register channels based on args / configured tokens
    let run_telegram = args.telegram || config.telegram_bot_token.is_some();
    let run_discord = args.discord || config.discord_bot_token.is_some();

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

    if !run_telegram && !run_discord {
        anyhow::bail!(
            "No channel token configured. Set TELEGRAM_BOT_TOKEN and/or DISCORD_BOT_TOKEN."
        );
    }

    let idle_future = bot.run();

    tokio::select! {
        result = idle_future => result,
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down");
            std::process::exit(0);
        }
    }
}
