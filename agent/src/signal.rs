//! State signal socket — push-based BUSY→IDLE transitions from tool scripts.
//!
//! Scripts (telegram_send, no_response, etc.) connect to a Unix socket and
//! send a line-delimited JSON message like:
//!
//!   {"session":"telegram:-12345","state":"IDLE"}
//!
//! The server broadcasts these signals to all subscribed actors so they know
//! the model has taken a deliberate response action.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// A state transition signal sent by a tool script.
#[derive(Debug, Clone, Deserialize)]
pub struct StateSignal {
    /// Session key, e.g. "telegram:-12345".
    pub session: String,
    /// Target state, e.g. "IDLE".
    pub state: String,
}

/// Well-known socket name under `<workspace>/agents/`.
pub const SIGNAL_SOCKET_NAME: &str = ".state";

/// Path inside the container where the signal socket is expected.
pub const CONTAINER_SIGNAL_SOCKET: &str = "/workspace/agents/.state";

/// Server that listens on a Unix socket for state signals from tool scripts.
pub struct StateSignalServer {
    tx: broadcast::Sender<StateSignal>,
    _task: tokio::task::JoinHandle<()>,
}

impl StateSignalServer {
    /// Start listening on `<base_dir>/.state_signal`.
    pub async fn start(base_dir: &Path) -> anyhow::Result<Self> {
        let socket_path = base_dir.join(SIGNAL_SOCKET_NAME);
        Self::start_at(socket_path).await
    }

    /// Start listening on a specific socket path.
    pub async fn start_at(socket_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let _ = tokio::fs::remove_file(&socket_path).await;

        let listener = UnixListener::bind(&socket_path)?;

        // Make socket world-writable so container (different uid) can connect.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &socket_path,
                std::fs::Permissions::from_mode(0o777),
            );
        }

        let (tx, _) = broadcast::channel::<StateSignal>(64);
        let tx_clone = tx.clone();

        info!(path = %socket_path.display(), "state signal socket started");

        let task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let tx = tx_clone.clone();
                        tokio::spawn(async move {
                            let reader = BufReader::new(stream);
                            let mut lines = reader.lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                let line = line.trim().to_string();
                                if line.is_empty() {
                                    continue;
                                }
                                match serde_json::from_str::<StateSignal>(&line) {
                                    Ok(signal) => {
                                        debug!(
                                            session = %signal.session,
                                            state = %signal.state,
                                            "received state signal"
                                        );
                                        let _ = tx.send(signal);
                                    }
                                    Err(e) => {
                                        warn!("invalid state signal: {e} — raw: {line}");
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        warn!("state signal accept error: {e}");
                    }
                }
            }
        });

        Ok(Self { tx, _task: task })
    }

    /// Subscribe to state signal events.
    pub fn subscribe(&self) -> broadcast::Receiver<StateSignal> {
        self.tx.subscribe()
    }
}
