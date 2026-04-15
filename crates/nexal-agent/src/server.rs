mod rpc;
mod services;
mod transport;

pub(crate) use services::{ExecServerHandler, ProcessEvent, ProcessEventBroadcaster};
pub use transport::DEFAULT_LISTEN_URL;
pub use transport::ExecServerListenUrlParseError;
#[cfg(test)]
pub(crate) use transport::start_server;

pub async fn run_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_main_with_listen_url(DEFAULT_LISTEN_URL).await
}

pub async fn run_main_with_listen_url(
    listen_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    transport::run_transport(listen_url).await
}
