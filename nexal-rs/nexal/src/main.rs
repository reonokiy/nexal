use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use clap::Subcommand;
use nexal_arg0::Arg0DispatchPaths;
use nexal_config_loader::LoaderOverrides;
use nexal_tui::Cli as TuiCli;
use nexal_agent::AgentPool;
use nexal_config::NexalConfig;
use nexal_state::StateDb;
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

    // Sync skill directories into workspace so codex discovers SKILL.md files
    sync_skills(&config).await?;

    // Build TUI CLI with nexal defaults
    let mut tui_cli = TuiCli::parse_from(["nexal"]);
    tui_cli.cwd = Some(config.workspace_dir.clone());

    // Tell codex to also look for SOUL.md as project-level instructions
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

/// Ensure skill directories are visible in the workspace so codex
/// discovers their SKILL.md files.
///
/// Priority for skills source:
/// 1. `NEXAL_SKILLS_DIR` env var
/// 2. `nexal/skills/` in the repo (development layout)
/// 3. Already present at `<workspace>/skills/`
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
    let db_path = config.workspace_dir.join("agents").join("nexal.db");
    let db = Arc::new(
        StateDb::open(&db_path)
            .await
            .context("opening state db")?,
    );

    info!("state db: {}", db_path.display());

    let pool = AgentPool::new(Arc::clone(&config));

    let run_telegram = args.telegram || config.telegram_bot_token.is_some();
    let run_discord = args.discord || config.discord_bot_token.is_some();

    let idle_future = async {
        match (run_telegram, run_discord) {
            (true, true) => {
                tokio::try_join!(
                    nexal_channel_telegram::run(
                        Arc::clone(&pool),
                        Arc::clone(&config),
                        Arc::clone(&db),
                    ),
                    nexal_channel_discord::run(
                        Arc::clone(&pool),
                        Arc::clone(&config),
                        Arc::clone(&db),
                    ),
                )?;
            }
            (true, false) => {
                nexal_channel_telegram::run(pool, config, db).await?;
            }
            (false, true) => {
                nexal_channel_discord::run(pool, config, db).await?;
            }
            (false, false) => {
                anyhow::bail!(
                    "No channel token configured. Set TELEGRAM_BOT_TOKEN and/or DISCORD_BOT_TOKEN."
                );
            }
        }
        Ok(())
    };

    tokio::select! {
        result = idle_future => result,
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down");
            std::process::exit(0);
        }
    }
}
