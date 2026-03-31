//! Nexal configuration — loaded from TOML + environment variables via figment.
//!
//! Config sources (lowest → highest priority):
//! 1. Built-in defaults
//! 2. `~/.nexal/config.toml`
//! 3. Environment variables prefixed with `NEXAL_`
//!
//! Environment variable mapping uses `__` for nesting:
//!   NEXAL_PROVIDERS__MOONSHOT__BASE_URL=https://api.moonshot.cn/v1
//!   NEXAL_DEBOUNCE_SECS=2.0

pub mod sandbox;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

const DEFAULT_SOUL: &str = r#"You are Nexal, a helpful AI assistant. You have access to a variety of tools to help you complete tasks.

Guidelines:
- Be concise and helpful
- Use tools when needed to accomplish tasks
- For long responses, break them into multiple shorter messages separated by blank lines
- Always respond in the language the user writes in
"#;

/// Complete nexal configuration.
///
/// Loaded from `~/.nexal/config.toml` + `NEXAL_*` environment variables.
/// LLM providers are configured under `[providers.<name>]`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NexalConfig {
    /// Override the nexal home directory (default: ~/.nexal)
    pub nexal_home: Option<PathBuf>,

    /// Workspace root directory
    pub workspace: PathBuf,

    /// Path to SOUL.md persona file
    pub soul_path: Option<PathBuf>,

    /// Telegram bot token
    pub telegram_bot_token: Option<String>,
    /// Allowed Telegram usernames
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub telegram_allow_from: Vec<String>,
    /// Allowed Telegram chat IDs
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub telegram_allow_chats: Vec<String>,

    /// Discord bot token
    pub discord_bot_token: Option<String>,
    /// Allowed Discord guild IDs
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub discord_allow_guilds: Vec<String>,

    /// Admin usernames
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub admins: Vec<String>,

    /// Debounce delay after mention (seconds)
    pub debounce_secs: f64,
    /// Delay for follow-up messages in active window (seconds)
    pub message_delay_secs: f64,
    /// Active conversation window (seconds)
    pub active_window_secs: f64,

    /// Sandbox backend: "podman" (default) or "none"
    pub sandbox: String,
    /// Podman container image
    pub sandbox_image: String,
    /// OCI runtime override (e.g. crun, kata)
    pub sandbox_runtime: Option<String>,
    /// Memory limit for sandbox containers
    pub sandbox_memory: String,
    /// CPU limit for sandbox containers
    pub sandbox_cpus: String,
    /// PID limit for sandbox containers
    pub sandbox_pids_limit: u32,
    /// Enable network access inside sandbox
    pub sandbox_network: bool,

    /// Path to skills directory
    pub skills_dir: Option<PathBuf>,

    /// LLM provider configurations
    pub providers: HashMap<String, ProviderConfig>,
}

/// Configuration for a single LLM provider.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ProviderConfig {
    /// Display name
    pub name: Option<String>,
    /// API base URL (e.g. "https://api.moonshot.cn/v1")
    pub base_url: Option<String>,
    /// Environment variable containing the API key
    pub env_key: Option<String>,
    /// Wire protocol: "responses" or "chat"
    pub wire_api: Option<String>,
    /// Enable thinking/reasoning mode (e.g. for Kimi)
    pub thinking_mode: bool,
    /// Custom HTTP headers (static)
    pub http_headers: Option<HashMap<String, String>>,
    /// Custom HTTP headers from environment variables
    pub env_http_headers: Option<HashMap<String, String>>,
    /// Custom query parameters
    pub query_params: Option<HashMap<String, String>>,
}

impl Default for NexalConfig {
    fn default() -> Self {
        let home = dirs_home();
        Self {
            nexal_home: None,
            workspace: home.join(".nexal").join("workspace"),
            soul_path: None,
            telegram_bot_token: None,
            telegram_allow_from: Vec::new(),
            telegram_allow_chats: Vec::new(),
            discord_bot_token: None,
            discord_allow_guilds: Vec::new(),
            admins: Vec::new(),
            debounce_secs: 1.0,
            message_delay_secs: 10.0,
            active_window_secs: 60.0,
            sandbox: "podman".to_string(),
            sandbox_image: "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13".to_string(),
            sandbox_runtime: None,
            sandbox_memory: "512m".to_string(),
            sandbox_cpus: "1.0".to_string(),
            sandbox_pids_limit: 256,
            sandbox_network: true,
            skills_dir: None,
            providers: HashMap::new(),
        }
    }
}

impl NexalConfig {
    /// Load config from defaults → TOML → env vars.
    pub fn from_env() -> Self {
        let home = dirs_home();
        let config_path = std::env::var("NEXAL_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".nexal"))
            .join("config.toml");

        let figment = Figment::new()
            .merge(Serialized::defaults(NexalConfig::default()))
            .merge(Toml::file(&config_path))
            .merge(
                Env::prefixed("NEXAL_")
                    .split("__")
                    // Map common env vars without prefix for convenience
                    .map(|key| {
                        match key.as_str() {
                            // Allow TELEGRAM_BOT_TOKEN (no NEXAL_ prefix)
                            _ => key.into(),
                        }
                    }),
            );

        // Also merge non-prefixed env vars for backward compatibility
        let figment = figment
            .merge(Env::raw().only(&[
                "TELEGRAM_BOT_TOKEN",
                "DISCORD_BOT_TOKEN",
                "SANDBOX_IMAGE",
                "SANDBOX_RUNTIME",
            ]).map(|key| {
                match key.as_str() {
                    "TELEGRAM_BOT_TOKEN" => "telegram_bot_token".into(),
                    "DISCORD_BOT_TOKEN" => "discord_bot_token".into(),
                    "SANDBOX_IMAGE" => "sandbox_image".into(),
                    "SANDBOX_RUNTIME" => "sandbox_runtime".into(),
                    _ => key.into(),
                }
            }));

        match figment.extract::<NexalConfig>() {
            Ok(mut config) => {
                if config.soul_path.is_none() {
                    config.soul_path = Some(config.workspace.join("agents").join("SOUL.md"));
                }
                // Normalize all comma-separated list fields at load time
                fn normalize_list(v: &[String]) -> Vec<String> {
                    v.iter()
                        .flat_map(|s| s.split(',').map(|a| a.trim().trim_matches('@').to_string()))
                        .filter(|a| !a.is_empty())
                        .collect()
                }
                config.admins = normalize_list(&config.admins);
                config.telegram_allow_from = normalize_list(&config.telegram_allow_from);
                config.telegram_allow_chats = normalize_list(&config.telegram_allow_chats);
                config.discord_allow_guilds = normalize_list(&config.discord_allow_guilds);
                config
            }
            Err(e) => {
                tracing::warn!("config load error (using defaults): {e}");
                Self::default()
            }
        }
    }

    // ── Convenience accessors (backward compatible) ──

    /// Workspace directory (renamed from workspace_dir)
    pub fn workspace_dir(&self) -> &PathBuf {
        &self.workspace
    }

    /// Effective SOUL.md path
    pub fn soul_path(&self) -> PathBuf {
        self.soul_path
            .clone()
            .unwrap_or_else(|| self.workspace.join("agents").join("SOUL.md"))
    }

    /// Sandbox backend enum
    pub fn sandbox_backend(&self) -> SandboxBackend {
        match self.sandbox.to_lowercase().as_str() {
            "none" | "off" | "disabled" => SandboxBackend::None,
            _ => SandboxBackend::Podman,
        }
    }

    /// Read SOUL.md + SOUL.override.md content.
    ///
    /// SOUL.md is the base persona (user-controlled, highest priority).
    /// SOUL.override.md is self-modifiable by the agent for style adjustments.
    /// Combined as: SOUL.md + "\n\n" + SOUL.override.md
    pub async fn load_soul(&self) -> String {
        let path = self.soul_path();
        let base = match tokio::fs::read_to_string(&path).await {
            Ok(content) if !content.trim().is_empty() => content,
            _ => {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&path, DEFAULT_SOUL).await;
                DEFAULT_SOUL.to_string()
            }
        };

        // Append agent's self-modified override if it exists
        let override_path = path.with_file_name("SOUL.override.md");
        let override_content = tokio::fs::read_to_string(&override_path)
            .await
            .unwrap_or_default();

        if override_content.trim().is_empty() {
            base
        } else {
            format!("{base}\n\n---\n\n{override_content}")
        }
    }

    pub fn is_admin(&self, username: &str) -> bool {
        self.admins.iter().any(|a| a.eq_ignore_ascii_case(username))
    }

    pub fn is_telegram_allowed_user(&self, username: &str) -> bool {
        self.telegram_allow_from.is_empty()
            || self.telegram_allow_from.iter().any(|u| u == username)
    }

    pub fn is_telegram_allowed_chat(&self, chat_id: &str) -> bool {
        self.telegram_allow_chats.is_empty()
            || self.telegram_allow_chats.iter().any(|c| c == chat_id)
    }

    pub fn is_discord_allowed_guild(&self, guild_id: &str) -> bool {
        self.discord_allow_guilds.is_empty()
            || self.discord_allow_guilds.iter().any(|g| g == guild_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Podman,
    None,
}

impl fmt::Display for SandboxBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Podman => write!(f, "podman"),
            Self::None => write!(f, "none"),
        }
    }
}

/// Deserialize a Vec<String> that also accepts integers (e.g. Telegram chat IDs).
fn deserialize_string_or_int_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrIntVec;

    impl<'de> de::Visitor<'de> for StringOrIntVec {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of strings or integers, or a comma-separated string")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<String>, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut v = Vec::new();
            while let Some(val) = seq.next_element::<serde_json::Value>()? {
                match val {
                    serde_json::Value::String(s) => v.push(s),
                    serde_json::Value::Number(n) => v.push(n.to_string()),
                    _ => v.push(val.to_string()),
                }
            }
            Ok(v)
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Vec<String>, E> {
            Ok(s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        }

        fn visit_i64<E: de::Error>(self, n: i64) -> Result<Vec<String>, E> {
            Ok(vec![n.to_string()])
        }

        fn visit_u64<E: de::Error>(self, n: u64) -> Result<Vec<String>, E> {
            Ok(vec![n.to_string()])
        }
    }

    deserializer.deserialize_any(StringOrIntVec)
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let cfg = NexalConfig::default();
        assert_eq!(cfg.sandbox, "podman");
        assert_eq!(cfg.debounce_secs, 1.0);
        assert!(cfg.providers.is_empty());
        assert!(cfg.sandbox_backend() == SandboxBackend::Podman);
    }

    #[test]
    fn figment_loads_providers_from_toml_string() {
        use figment::providers::{Format, Serialized, Toml};

        let toml_str = r#"
            [providers.test]
            base_url = "https://example.com/v1"
            wire_api = "chat"
            thinking_mode = true
        "#;

        let config: NexalConfig = Figment::new()
            .merge(Serialized::defaults(NexalConfig::default()))
            .merge(Toml::string(toml_str))
            .extract()
            .unwrap();

        assert_eq!(config.providers.len(), 1);
        let p = &config.providers["test"];
        assert_eq!(p.base_url.as_deref(), Some("https://example.com/v1"));
        assert_eq!(p.wire_api.as_deref(), Some("chat"));
        assert!(p.thinking_mode);
    }

    #[test]
    fn sandbox_backend_parsing() {
        let mut cfg = NexalConfig::default();
        assert_eq!(cfg.sandbox_backend(), SandboxBackend::Podman);

        cfg.sandbox = "none".to_string();
        assert_eq!(cfg.sandbox_backend(), SandboxBackend::None);

        cfg.sandbox = "off".to_string();
        assert_eq!(cfg.sandbox_backend(), SandboxBackend::None);
    }
}
