//! Stubbed login server module.
//!
//! The interactive OAuth callback server has been removed. All public types and
//! functions are retained so downstream code continues to compile, but attempting
//! to start the login server returns an error directing the user to API-key auth.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::AuthCredentialsStoreMode;

/// Options for launching the local login callback server.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub nexal_home: PathBuf,
    pub client_id: String,
    pub issuer: String,
    pub port: u16,
    pub open_browser: bool,
    pub force_state: Option<String>,
    pub forced_chatgpt_workspace_id: Option<String>,
    pub cli_auth_credentials_store_mode: AuthCredentialsStoreMode,
}

const DEFAULT_ISSUER: &str = "https://auth.openai.com";
const DEFAULT_PORT: u16 = 1455;

impl ServerOptions {
    /// Creates a server configuration with the default issuer and port.
    pub fn new(
        nexal_home: PathBuf,
        client_id: String,
        forced_chatgpt_workspace_id: Option<String>,
        cli_auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> Self {
        Self {
            nexal_home,
            client_id,
            issuer: DEFAULT_ISSUER.to_string(),
            port: DEFAULT_PORT,
            open_browser: true,
            force_state: None,
            forced_chatgpt_workspace_id,
            cli_auth_credentials_store_mode,
        }
    }
}

/// Handle for a running login callback server.
pub struct LoginServer {
    pub auth_url: String,
    pub actual_port: u16,
    server_handle: tokio::task::JoinHandle<io::Result<()>>,
    shutdown_handle: ShutdownHandle,
}

impl LoginServer {
    /// Waits for the login callback loop to finish.
    pub async fn block_until_done(self) -> io::Result<()> {
        self.server_handle
            .await
            .map_err(|err| io::Error::other(format!("login server thread panicked: {err:?}")))?
    }

    /// Requests shutdown of the callback server.
    pub fn cancel(&self) {
        self.shutdown_handle.shutdown();
    }

    /// Returns a cloneable cancel handle for the running server.
    pub fn cancel_handle(&self) -> ShutdownHandle {
        self.shutdown_handle.clone()
    }
}

/// Handle used to signal the login server loop to exit.
#[derive(Clone, Debug)]
pub struct ShutdownHandle {
    shutdown_notify: Arc<tokio::sync::Notify>,
}

impl ShutdownHandle {
    /// Signals the login loop to terminate.
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_waiters();
    }
}

/// Stub: the interactive ChatGPT OAuth login server has been removed.
///
/// Returns an error directing the caller to use API-key authentication.
pub fn run_login_server(_opts: ServerOptions) -> io::Result<LoginServer> {
    Err(io::Error::other(
        "ChatGPT OAuth login is not supported. Use an API key instead.",
    ))
}
