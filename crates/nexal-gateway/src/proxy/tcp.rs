//! Direct TCP proxy — forwards external connections to container ports.
//!
//! When a stream proxy is registered, the gateway:
//!   1. Looks up the agent's port_map to find the reachable address
//!      for the requested container port.
//!   2. Starts a TCP listener on a random host port.
//!   3. For each incoming connection, opens a TCP connection to the
//!      container's mapped address and does bidirectional copy.
//!
//! Zero encoding overhead — raw bytes flow directly.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

#[derive(Debug)]
struct TcpProxyEntry {
    /// Gateway-side listen address (e.g. "127.0.0.1:49201").
    listen_addr: String,
    cancel: CancellationToken,
}

#[derive(Default)]
pub struct TcpProxyRegistry {
    /// (agent_id, name) → entry.
    inner: RwLock<HashMap<(String, String), TcpProxyEntry>>,
}

impl TcpProxyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a TCP proxy listener and return the listen address.
    pub async fn register(
        &self,
        agent_id: String,
        name: String,
        upstream_addr: String,
    ) -> Result<String, String> {
        // Bind to a random port.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("bind tcp proxy: {e}"))?;
        let listen_addr = listener
            .local_addr()
            .map_err(|e| format!("local_addr: {e}"))?
            .to_string();

        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();
        let upstream = upstream_addr.clone();
        let proxy_name = format!("{agent_id}/{name}");

        info!(
            proxy = %proxy_name,
            listen = %listen_addr,
            upstream = %upstream,
            "tcp stream proxy registered"
        );

        tokio::spawn(async move {
            run_tcp_proxy(listener, upstream, proxy_name, cancel_for_task).await;
        });

        let key = (agent_id, name);
        let mut inner = self.inner.write().await;
        // Cancel existing proxy for this key if any.
        if let Some(old) = inner.remove(&key) {
            old.cancel.cancel();
        }
        inner.insert(
            key,
            TcpProxyEntry {
                listen_addr: listen_addr.clone(),
                cancel,
            },
        );

        Ok(listen_addr)
    }

    /// Stop a specific proxy. Returns true if it existed.
    pub async fn unregister(&self, agent_id: &str, name: &str) -> bool {
        let key = (agent_id.to_string(), name.to_string());
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.remove(&key) {
            entry.cancel.cancel();
            info!(
                proxy = format!("{agent_id}/{name}"),
                "tcp stream proxy unregistered"
            );
            true
        } else {
            false
        }
    }

    /// Drop all proxies for an agent (called on agent kill).
    pub async fn cleanup_for_agent(&self, agent_id: &str) -> usize {
        let mut inner = self.inner.write().await;
        let to_remove: Vec<(String, String)> = inner
            .keys()
            .filter(|(a, _)| a == agent_id)
            .cloned()
            .collect();
        let count = to_remove.len();
        for key in to_remove {
            if let Some(entry) = inner.remove(&key) {
                entry.cancel.cancel();
            }
        }
        count
    }

    /// Get the listen address for a proxy.
    pub async fn get_listen_addr(&self, agent_id: &str, name: &str) -> Option<String> {
        let key = (agent_id.to_string(), name.to_string());
        self.inner.read().await.get(&key).map(|e| e.listen_addr.clone())
    }
}

pub type SharedTcpProxyRegistry = Arc<TcpProxyRegistry>;

async fn run_tcp_proxy(
    listener: TcpListener,
    upstream: String,
    proxy_name: String,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((client, peer)) => {
                        debug!(proxy = %proxy_name, peer = %peer, "tcp proxy: accepted");
                        let upstream = upstream.clone();
                        let name = proxy_name.clone();
                        tokio::spawn(async move {
                            if let Err(e) = relay(client, &upstream).await {
                                debug!(proxy = %name, "tcp proxy relay ended: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        error!(proxy = %proxy_name, "tcp proxy accept error: {e}");
                        break;
                    }
                }
            }
            _ = cancel.cancelled() => {
                debug!(proxy = %proxy_name, "tcp proxy cancelled");
                break;
            }
        }
    }
}

async fn relay(
    mut client: tokio::net::TcpStream,
    upstream_addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut upstream = tokio::net::TcpStream::connect(upstream_addr).await?;
    let (mut cr, mut cw) = client.split();
    let (mut ur, mut uw) = upstream.split();

    tokio::select! {
        r = io::copy(&mut cr, &mut uw) => { r?; }
        r = io::copy(&mut ur, &mut cw) => { r?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_unregister() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Start a simple echo server as the "upstream".
        let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = echo.accept().await {
                let mut buf = vec![0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n > 0 {
                    stream.write_all(&buf[..n]).await.unwrap();
                }
            }
        });

        let registry = TcpProxyRegistry::new();
        let listen = registry
            .register("agent-1".into(), "rdp".into(), echo_addr)
            .await
            .expect("register should succeed");

        // Connect through the proxy and send data.
        let mut client = tokio::net::TcpStream::connect(&listen).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        // Read back the echoed data.
        let mut buf = vec![0u8; 5];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");

        assert!(registry.unregister("agent-1", "rdp").await);
        assert!(!registry.unregister("agent-1", "rdp").await);
    }

    #[tokio::test]
    async fn cleanup_for_agent_drops_all() {
        let registry = TcpProxyRegistry::new();
        // We can't connect, but registration itself should work.
        let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = echo.local_addr().unwrap().to_string();
        registry.register("a".into(), "rdp".into(), addr.clone()).await.unwrap();
        registry.register("a".into(), "cdp".into(), addr.clone()).await.unwrap();
        registry.register("b".into(), "rdp".into(), addr).await.unwrap();

        assert_eq!(registry.cleanup_for_agent("a").await, 2);
        assert!(registry.get_listen_addr("a", "rdp").await.is_none());
        assert!(registry.get_listen_addr("b", "rdp").await.is_some());
    }
}
