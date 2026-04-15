//! In-memory registry of (agent_id, name) → ProxyEntry.
//!
//! `token` is the opaque identifier baked into the URL the agent uses.
//! It's a v4 UUID with hyphens stripped — short enough to look clean
//! in a URL, long enough to defeat enumeration.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProxyEntry {
    pub agent_id: String,
    pub name: String,
    pub upstream_url: String,
    pub headers: HashMap<String, String>,
    pub token: String,
}

#[derive(Default)]
pub struct ProxyRegistry {
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    /// token → entry
    by_token: HashMap<String, ProxyEntry>,
    /// (agent_id, name) → token
    index: HashMap<(String, String), String>,
}

impl ProxyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new (agent_id, name) → upstream mapping. If one
    /// already exists for that key, replace it (and discard the old
    /// token so further use of it 404s).
    pub async fn register(
        &self,
        agent_id: String,
        name: String,
        upstream_url: String,
        headers: HashMap<String, String>,
    ) -> ProxyEntry {
        let mut inner = self.inner.write().await;
        let key = (agent_id.clone(), name.clone());
        if let Some(old_token) = inner.index.remove(&key) {
            inner.by_token.remove(&old_token);
        }
        let token = generate_token();
        let entry = ProxyEntry {
            agent_id,
            name,
            upstream_url,
            headers,
            token: token.clone(),
        };
        inner.index.insert(key, token.clone());
        inner.by_token.insert(token, entry.clone());
        entry
    }

    /// Drop a single registration. Returns `true` if something was removed.
    pub async fn unregister(&self, agent_id: &str, name: &str) -> bool {
        let mut inner = self.inner.write().await;
        let key = (agent_id.to_string(), name.to_string());
        if let Some(token) = inner.index.remove(&key) {
            inner.by_token.remove(&token);
            true
        } else {
            false
        }
    }

    /// Drop everything owned by `agent_id`. Called when the agent is killed.
    pub async fn cleanup_for_agent(&self, agent_id: &str) -> usize {
        let mut inner = self.inner.write().await;
        let to_remove: Vec<(String, String)> = inner
            .index
            .keys()
            .filter(|(a, _)| a == agent_id)
            .cloned()
            .collect();
        let count = to_remove.len();
        for key in to_remove {
            if let Some(token) = inner.index.remove(&key) {
                inner.by_token.remove(&token);
            }
        }
        count
    }

    pub async fn lookup(&self, token: &str) -> Option<ProxyEntry> {
        self.inner.read().await.by_token.get(token).cloned()
    }
}

pub type SharedProxyRegistry = Arc<ProxyRegistry>;

fn generate_token() -> String {
    Uuid::new_v4().simple().to_string()
}
