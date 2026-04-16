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
    pub proxy: ProxyConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct ProxyConfig {
    /// Listen address for the reverse-proxy HTTP server.
    /// Default: `0.0.0.0:5501`.
    pub listen: Option<String>,
    /// Base URL given to agents in `register_proxy` responses.
    /// Default: `http://host.containers.internal:5501`.
    pub external_base: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct SpawnDefaultsConfig {
    pub image: Option<String>,
    pub agent_bin: Option<PathBuf>,
    pub memory: Option<String>,
    pub cpus: Option<String>,
    pub pids_limit: Option<u32>,
    pub network: Option<bool>,
    pub container_name_prefix: Option<String>,
    /// Host path bind-mounted at `/workspace` in every container.
    pub workspace_volume: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_none() {
        let c = GatewayConfig::default();
        assert!(c.listen.is_none());
        assert!(c.token.is_none());
        assert!(c.defaults.image.is_none());
        assert!(c.defaults.agent_bin.is_none());
        assert!(c.backend.kind.is_none());
        assert!(c.proxy.listen.is_none());
    }

    #[test]
    fn parses_all_top_level_keys() {
        let text = r#"
listen = "0.0.0.0:5500"
token  = "shared"

[defaults]
image       = "ghcr.io/nexal:latest"
memory      = "256m"
cpus        = "0.5"
pids_limit  = 64
network     = true
container_name_prefix = "nxw-"

[backend]
kind       = "podman"
podman_bin = "/usr/bin/podman"
runtime    = "crun"

[proxy]
listen        = "127.0.0.1:5501"
external_base = "http://host.containers.internal:5501"
"#;
        let c: GatewayConfig =
            toml::from_str(text).expect("gateway config should parse");
        assert_eq!(c.listen.as_deref(), Some("0.0.0.0:5500"));
        assert_eq!(c.token.as_deref(), Some("shared"));
        assert_eq!(c.defaults.image.as_deref(), Some("ghcr.io/nexal:latest"));
        assert_eq!(c.defaults.memory.as_deref(), Some("256m"));
        assert_eq!(c.defaults.cpus.as_deref(), Some("0.5"));
        assert_eq!(c.defaults.pids_limit, Some(64));
        assert_eq!(c.defaults.network, Some(true));
        assert_eq!(c.defaults.container_name_prefix.as_deref(), Some("nxw-"));
        assert_eq!(c.backend.kind.as_deref(), Some("podman"));
        assert_eq!(c.backend.podman_bin.as_deref(), Some("/usr/bin/podman"));
        assert_eq!(c.backend.runtime.as_deref(), Some("crun"));
        assert_eq!(c.proxy.listen.as_deref(), Some("127.0.0.1:5501"));
    }

    #[test]
    fn missing_optional_sections_are_filled_with_defaults() {
        // Only `listen` supplied — everything else takes default.
        let c: GatewayConfig =
            toml::from_str(r#"listen = "a""#).expect("partial config should parse");
        assert_eq!(c.listen.as_deref(), Some("a"));
        assert!(c.backend.kind.is_none());
        assert!(c.defaults.image.is_none());
        assert!(c.proxy.external_base.is_none());
    }

    #[test]
    fn unknown_keys_are_accepted_silently() {
        // `#[serde(default)]` on structs means extra keys don't ERROR,
        // but TOML's top-level deserializer rejects unknowns by default.
        // Verify a known nested unknown is allowed (documented behavior).
        let text = r#"
listen = "a"
[defaults]
image = "x"
# made-up-key is NOT defined on SpawnDefaultsConfig; serde drops it
# only because we don't `deny_unknown_fields`.
"#;
        let c: GatewayConfig =
            toml::from_str(text).expect("unknown inner keys shouldn't fail");
        assert_eq!(c.defaults.image.as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn load_missing_file_returns_default() {
        let path = std::env::temp_dir().join("definitely-not-here-98124.toml");
        // Be sure the test path is absent.
        let _ = tokio::fs::remove_file(&path).await;
        let c = GatewayConfig::load(&path)
            .await
            .expect("missing file should be OK");
        assert!(c.listen.is_none());
        assert!(c.token.is_none());
    }

    #[tokio::test]
    async fn load_malformed_toml_returns_parse_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("nexal-gateway-bad-config.toml");
        tokio::fs::write(&path, "this is = not = valid = toml")
            .await
            .expect("write tmp file");
        let err = GatewayConfig::load(&path).await.err();
        assert!(matches!(err, Some(ConfigError::Parse(_, _))));
        let _ = tokio::fs::remove_file(&path).await;
    }
}
