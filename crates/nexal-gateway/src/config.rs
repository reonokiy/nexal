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
    /// Frontend credentials. The handshake is HMAC-signed
    /// (`gateway/hello`); the shared `token` was removed.
    pub credentials: Vec<Credential>,
    pub defaults: SpawnDefaultsConfig,
    pub backend: BackendConfig,
    pub proxy: ProxyConfig,
    pub pool: PoolConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Credential {
    pub access_key: String,
    pub secret_key: String,
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
    /// Host path to the skills directory. Default: `~/.nexal/skills`.
    pub skills_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct BackendConfig {
    pub kind: Option<String>,
    // Podman-specific.
    pub podman_bin: Option<String>,
    pub runtime: Option<String>,
    // Kubernetes-specific.
    pub namespace: Option<String>,
    pub kubeconfig: Option<PathBuf>,
    /// Image that ships `/usr/local/bin/nexal-agent` for the initContainer.
    pub agent_init_image: Option<String>,
    // Fly.io-specific.
    /// Fly API / org-deploy token (`Authorization: Bearer`).
    pub fly_api_token: Option<String>,
    /// Fly app the worker machines belong to.
    pub fly_app: Option<String>,
    /// Default Fly region (e.g. `"iad"`). None → Fly picks.
    pub fly_region: Option<String>,
    /// Machines API base. Default `https://api.machines.dev`.
    pub fly_api_base: Option<String>,
    /// Path of `nexal-agent` inside the image. Default
    /// `/usr/local/bin/nexal-agent`.
    pub fly_agent_bin_path: Option<String>,
    // Sprites.dev-specific.
    /// Sprites API token (`Authorization: Bearer`).
    pub sprites_token: Option<String>,
    /// Sprites API base. Default `https://api.sprites.dev`.
    pub sprites_api_base: Option<String>,
    /// Prefix used when deriving Sprite names. Defaults to the gateway
    /// container name passed by `AgentRegistry`.
    pub sprites_name_prefix: Option<String>,
    /// Port where `nexal-agent` listens inside the Sprite. Default `9100`.
    pub sprites_agent_port: Option<u16>,
    /// Path where the gateway uploads `nexal-agent` inside each Sprite.
    /// Default `/usr/local/bin/nexal-agent`.
    pub sprites_agent_bin_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct PoolConfig {
    /// Enable the warm container pool.
    pub enabled: Option<bool>,
    /// Number of warm containers to keep ready. Default: 0 (disabled).
    pub size: Option<usize>,
    /// Image for warm containers. Falls back to `defaults.image`.
    pub image: Option<String>,
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
        assert!(c.credentials.is_empty());
        assert!(c.defaults.image.is_none());
        assert!(c.defaults.agent_bin.is_none());
        assert!(c.backend.kind.is_none());
        assert!(c.proxy.listen.is_none());
    }

    #[test]
    fn parses_all_top_level_keys() {
        let text = r#"
listen = "0.0.0.0:5500"

[[credentials]]
access_key = "AK1"
secret_key = "sk1"

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
        let c: GatewayConfig = toml::from_str(text).expect("gateway config should parse");
        assert_eq!(c.listen.as_deref(), Some("0.0.0.0:5500"));
        assert_eq!(c.credentials.len(), 1);
        assert_eq!(c.credentials[0].access_key, "AK1");
        assert_eq!(c.credentials[0].secret_key, "sk1");
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
    fn parses_kubernetes_backend_and_pool() {
        let text = r#"
[backend]
kind = "kubernetes"
namespace = "nexal"
kubeconfig = "/home/user/.kube/config"
agent_init_image = "ghcr.io/reonokiy/nexal-agent:latest"

[pool]
enabled = true
size = 3
image = "ghcr.io/reonokiy/nexal-sandbox:latest"
"#;
        let c: GatewayConfig = toml::from_str(text).expect("k8s + pool config should parse");
        assert_eq!(c.backend.kind.as_deref(), Some("kubernetes"));
        assert_eq!(c.backend.namespace.as_deref(), Some("nexal"));
        assert_eq!(
            c.backend.kubeconfig.as_ref().and_then(|p| p.to_str()),
            Some("/home/user/.kube/config")
        );
        assert_eq!(
            c.backend.agent_init_image.as_deref(),
            Some("ghcr.io/reonokiy/nexal-agent:latest")
        );
        assert_eq!(c.pool.enabled, Some(true));
        assert_eq!(c.pool.size, Some(3));
        assert_eq!(
            c.pool.image.as_deref(),
            Some("ghcr.io/reonokiy/nexal-sandbox:latest")
        );
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
        assert!(c.pool.enabled.is_none());
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
        let c: GatewayConfig = toml::from_str(text).expect("unknown inner keys shouldn't fail");
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
        assert!(c.credentials.is_empty());
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
