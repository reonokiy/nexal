//! Warm container pool — pre-creates containers so `spawn` can skip
//! the cold-start latency.
//!
//! The pool maintains up to `target_size` idle containers. When
//! `AgentRegistry::spawn` is called, it first tries `pool.take()` —
//! if a warm container is available, it's returned immediately.
//! Otherwise the normal `backend.ensure()` path is used.
//!
//! A background task (`replenish_loop`) tops the pool back up whenever
//! a container is taken. Containers are never recycled: once used they
//! are destroyed on `kill`, and the pool creates fresh ones.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::backend::{ContainerHandle, ContainerSpec, SharedBackend};

pub struct WarmPool {
    backend: SharedBackend,
    image: String,
    agent_bin: PathBuf,
    memory: Option<String>,
    cpus: Option<String>,
    pids_limit: Option<u32>,
    network: bool,
    name_prefix: String,
    target_size: usize,
    counter: AtomicU64,
    available: Mutex<Vec<ContainerHandle>>,
}

impl WarmPool {
    pub fn new(
        backend: SharedBackend,
        cfg: WarmPoolConfig,
        name_prefix: String,
        agent_bin: PathBuf,
        memory: Option<String>,
        cpus: Option<String>,
        pids_limit: Option<u32>,
        network: bool,
    ) -> Self {
        Self {
            backend,
            image: cfg.image,
            agent_bin,
            memory,
            cpus,
            pids_limit,
            network,
            name_prefix,
            target_size: cfg.size,
            counter: AtomicU64::new(0),
            available: Mutex::new(Vec::new()),
        }
    }

    /// Take a warm container from the pool, or `None` if empty.
    pub async fn take(&self) -> Option<ContainerHandle> {
        let mut pool = self.available.lock().await;
        let handle = pool.pop();
        if let Some(ref h) = handle {
            debug!("pool: handed out warm container {}", h.name);
        }
        handle
    }

    /// Current number of idle containers in the pool.
    pub async fn available_count(&self) -> usize {
        self.available.lock().await.len()
    }

    /// Create one warm container and add it to the pool.
    /// Returns `true` if the container was created and added.
    async fn create_one(&self) -> bool {
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        let name = format!("{}pool-{seq}", self.name_prefix);
        let spec = ContainerSpec {
            name: name.clone(),
            image: self.image.clone(),
            env: HashMap::new(),
            labels: {
                let mut m = HashMap::new();
                m.insert("nexal.pool".to_string(), "warm".to_string());
                m
            },
            agent_bin: self.agent_bin.clone(),
            memory: self.memory.clone(),
            cpus: self.cpus.clone(),
            pids_limit: self.pids_limit,
            network: self.network,
            workspace_volume: None,
            extra_ports: Vec::new(),
            fuse: true,
        };
        match self.backend.ensure(spec).await {
            Ok(handle) => {
                self.available.lock().await.push(handle);
                debug!("pool: created warm container {name}");
                true
            }
            Err(e) => {
                warn!("pool: failed to create warm container {name}: {e}");
                false
            }
        }
    }

    /// Fill the pool up to `target_size`. Called at startup and after
    /// a container is taken.
    pub async fn replenish(&self) {
        loop {
            let current = self.available.lock().await.len();
            if current >= self.target_size {
                break;
            }
            if !self.create_one().await {
                // Back off on failure to avoid tight retry loops.
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    /// Destroy all idle containers in the pool (called on shutdown).
    pub async fn drain(&self) {
        let handles: Vec<ContainerHandle> = {
            let mut pool = self.available.lock().await;
            pool.drain(..).collect()
        };
        for handle in handles {
            if let Err(e) = self.backend.destroy(&handle.name).await {
                warn!("pool: drain failed for {}: {e}", handle.name);
            }
        }
    }
}

/// Spawn a background task that keeps the pool topped up.
pub fn start_replenish_loop(pool: Arc<WarmPool>) {
    tokio::spawn(async move {
        info!(
            "pool: replenish loop started (target_size={})",
            pool.target_size
        );
        // Initial fill.
        pool.replenish().await;
        info!(
            "pool: initial fill complete ({} ready)",
            pool.available_count().await
        );
        // Then check periodically.
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            pool.replenish().await;
        }
    });
}

/// Configuration for the warm pool, deserialized from `[pool]` in
/// `gateway.toml`.
#[derive(Debug, Clone)]
pub struct WarmPoolConfig {
    /// Number of warm containers to keep ready.
    pub size: usize,
    /// Image for warm containers (should match the default spawn image).
    pub image: String,
}
