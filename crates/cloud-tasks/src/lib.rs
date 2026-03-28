//! Stubbed-out cloud-tasks crate.
//!
//! The ChatGPT-backend cloud task queue TUI has been removed.
//! Only the CLI type and entry-point are retained so that the top-level
//! binary continues to compile.

mod cli;
pub use cli::Cli;

use std::path::PathBuf;

/// Entry point for the `codex cloud` subcommand.
///
/// This is a no-op stub -- the cloud-task integration has been removed.
pub async fn run_main(_cli: Cli, _nexal_linux_sandbox_exe: Option<PathBuf>) -> anyhow::Result<()> {
    anyhow::bail!("Cloud tasks support has been removed from this build.")
}
