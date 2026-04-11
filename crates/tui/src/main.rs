#![allow(clippy::print_stdout, clippy::print_stderr)]

use nexal_arg0::Arg0DispatchPaths;
use nexal_core::config_loader::LoaderOverrides;
use nexal_tui::Cli;
use clap::Parser;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    let result = nexal_tui::run_main(
        cli,
        Arg0DispatchPaths::default(),
        LoaderOverrides::default(),
    )
    .await?;
    std::process::exit(result.exit_code());
}
