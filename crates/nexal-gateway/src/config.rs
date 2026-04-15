//! Gateway config — `~/.nexal/gateway.toml` overlay over built-in
//! defaults, with optional CLI/env overrides applied later by the
//! binary entrypoint.
//!
//! Example file:
//!
//! ```toml
//! listen = "127.0.0.1:5500"
//! token  = "shared-secret"
//!
//! [defaults]
//! image       = "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13"
//! agent_bin   = "/home/lean/i/nexal/target/release/nexal-agent"
//! workspace   = "/home/lean/scratch"
//! memory      = "512m"
//! cpus        = "1.0"
//! pids_limit  = 256
//! network     = true
//! container_name_prefix = "nexal-worker-"
//!
//! [backend]
//! kind     = "podman"
//! podman_bin = "podman"
//! runtime  = "crun"
//! ```

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct GatewayConfig {
    pub listen: Option<String>,
    pub token: Option<String>,
    pub defaults: SpawnDefaultsConfig,
    pub backend: BackendConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct SpawnDefaultsConfig {
    pub image: Option<String>,
    pub agent_bin: Option<PathBuf>,
    pub workspace: Option<String>,
    pub memory: Option<String>,
    pub cpus: Option<String>,
    pub pids_limit: Option<u32>,
    pub network: Option<bool>,
    pub container_name_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct BackendConfig {
    pub kind: Option<String>,
    pub podman_bin: Option<String>,
    pub runtime: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("parse {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
}

impl GatewayConfig {
    pub fn default_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".nexal").join("gateway.toml"))
    }

    /// Load from a path, ignoring missing files (returns default).
    pub async fn load(path: &PathBuf) -> Result<Self, ConfigError> {
        match tokio::fs::read_to_string(path).await {
            Ok(text) => toml::from_str::<GatewayConfig>(&text)
                .map_err(|e| ConfigError::Parse(path.clone(), e)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(ConfigError::Io(path.clone(), err)),
        }
    }
}
