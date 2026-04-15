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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_returns_distinct_tokens() {
        let reg = ProxyRegistry::new();
        let a = reg
            .register("agent-1".into(), "jina".into(), "https://jina".into(), HashMap::new())
            .await;
        let b = reg
            .register("agent-1".into(), "openai".into(), "https://oai".into(), HashMap::new())
            .await;
        assert_ne!(a.token, b.token);
    }

    #[tokio::test]
    async fn register_same_key_replaces_token() {
        let reg = ProxyRegistry::new();
        let first = reg
            .register("a".into(), "jina".into(), "https://jina".into(), HashMap::new())
            .await;
        let second = reg
            .register("a".into(), "jina".into(), "https://jina2".into(), HashMap::new())
            .await;
        assert_ne!(first.token, second.token, "token should rotate on replace");
        assert!(
            reg.lookup(&first.token).await.is_none(),
            "old token should 404"
        );
        let looked_up = reg.lookup(&second.token).await.unwrap();
        assert_eq!(looked_up.upstream_url, "https://jina2");
    }

    #[tokio::test]
    async fn unregister_only_removes_named_entry() {
        let reg = ProxyRegistry::new();
        let keep = reg
            .register("a".into(), "jina".into(), "u".into(), HashMap::new())
            .await;
        let drop = reg
            .register("a".into(), "openai".into(), "u".into(), HashMap::new())
            .await;
        assert!(reg.unregister("a", "openai").await);
        assert!(reg.lookup(&drop.token).await.is_none());
        assert!(reg.lookup(&keep.token).await.is_some());
    }

    #[tokio::test]
    async fn unregister_missing_returns_false() {
        let reg = ProxyRegistry::new();
        assert!(!reg.unregister("nobody", "jina").await);
    }

    #[tokio::test]
    async fn cleanup_for_agent_drops_all_its_entries() {
        let reg = ProxyRegistry::new();
        let _ = reg
            .register("a".into(), "jina".into(), "u".into(), HashMap::new())
            .await;
        let _ = reg
            .register("a".into(), "openai".into(), "u".into(), HashMap::new())
            .await;
        let other = reg
            .register("b".into(), "jina".into(), "u".into(), HashMap::new())
            .await;
        let dropped = reg.cleanup_for_agent("a").await;
        assert_eq!(dropped, 2);
        // Other agent's entry survives.
        assert!(reg.lookup(&other.token).await.is_some());
        // Re-register after cleanup works.
        let reborn = reg
            .register("a".into(), "jina".into(), "u".into(), HashMap::new())
            .await;
        assert_eq!(reborn.agent_id, "a");
    }

    #[tokio::test]
    async fn lookup_unknown_token_returns_none() {
        let reg = ProxyRegistry::new();
        assert!(reg.lookup("deadbeef").await.is_none());
    }

    #[tokio::test]
    async fn register_preserves_headers_and_upstream() {
        let reg = ProxyRegistry::new();
        let mut hdrs = HashMap::new();
        hdrs.insert("Authorization".into(), "Bearer x".into());
        let entry = reg
            .register(
                "a".into(),
                "jina".into(),
                "https://api.jina.ai".into(),
                hdrs.clone(),
            )
            .await;
        let got = reg.lookup(&entry.token).await.unwrap();
        assert_eq!(got.agent_id, "a");
        assert_eq!(got.name, "jina");
        assert_eq!(got.upstream_url, "https://api.jina.ai");
        assert_eq!(got.headers, hdrs);
    }
}
