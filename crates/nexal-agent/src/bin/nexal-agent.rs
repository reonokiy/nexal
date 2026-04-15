use clap::Parser;

#[derive(Debug, Parser)]
struct AgentArgs {
    /// Transport endpoint URL. Supported values: `ws://IP:PORT` (default).
    #[arg(
        long = "listen",
        value_name = "URL",
        default_value = nexal_agent::DEFAULT_LISTEN_URL
    )]
    listen: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = AgentArgs::parse();
    nexal_agent::run_main_with_listen_url(&args.listen).await
}
