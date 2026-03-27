use std::path::PathBuf;

use clap::Parser;
use nexal_utils_cli::CliConfigOverrides;

/// Applies the latest diff from a Nexal agent task.
#[derive(Debug, Parser)]
pub struct ApplyCommand {
    pub task_id: String,

    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,
}

pub async fn run_apply_command(
    _apply_cli: ApplyCommand,
    _cwd: Option<PathBuf>,
) -> anyhow::Result<()> {
    anyhow::bail!("ChatGPT apply command is not available in this build")
}
