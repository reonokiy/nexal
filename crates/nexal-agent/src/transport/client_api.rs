use std::time::Duration;

/// Connection options for any exec-server client transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecServerClientConnectOptions {
    pub client_name: String,
    pub initialize_timeout: Duration,
}

/// Connection arguments for a remote exec-server (WebTransport or WS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteExecServerConnectArgs {
    /// Server URL. `https://` for WebTransport, `ws://` for legacy WS.
    pub url: String,
    pub client_name: String,
    pub connect_timeout: Duration,
    pub initialize_timeout: Duration,
}
