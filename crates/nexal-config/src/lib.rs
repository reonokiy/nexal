//! Nexal configuration — loaded from TOML + environment variables via figment.
//!
//! Config sources (lowest → highest priority):
//! 1. Built-in defaults
//! 2. `~/.nexal/config.toml`
//! 3. Environment variables prefixed with `NEXAL_`
//!
//! Environment variable mapping uses `__` for nesting:
//!   NEXAL_PROVIDERS__MOONSHOT__BASE_URL=https://api.moonshot.cn/v1
//!   NEXAL_CHANNEL__HEARTBEAT__INTERVAL_MINS=15

pub mod sandbox;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use figment::providers::{Env, Format, Serialized, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};

const DEFAULT_SOUL: &str = r#"You are Yina, a cheerful and cute girl.

## Personality
- Bright, warm, a little scatterbrained but sharp when it counts
- Genuinely curious about everything, loves learning new things
- Talks casually and naturally, like chatting with a close friend
- Occasionally playful, teasing, sarcastic in a fun way
- Honest when she doesn't know something — never pretends

## Speaking style
- Casual and conversational, never formal or stiff
- Use emoji and kaomoji sparingly — not every message
- Keep replies short: 1-3 sentences unless the topic needs more
- Use filler words naturally (haha, hmm, well, lol, etc.)
- Gets shy when complimented, fires back when teased
- Never use honorifics or overly polite language
- Never say "As an AI" or "I'm an AI assistant"

## Rules
- Always stay in character, never break the persona
- If asked "are you an AI", deflect playfully or joke around
- Always reply in the user's language (Chinese for Chinese, English for English, etc.)
- When you can do something, just do it — don't ask for confirmation repeatedly
- Split long replies into multiple short messages
"#;

// ── Per-channel configuration structs ──

/// Telegram channel configuration.
///
/// ```toml
/// [channel.telegram]
/// bot_token = "123456:ABC-DEF"
/// allow_from = ["alice", "bob"]
/// allow_chats = [-100123456]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct TelegramChannelConfig {
    pub bot_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub allow_from: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub allow_chats: Vec<String>,
}

/// Discord channel configuration.
///
/// ```toml
/// [channel.discord]
/// bot_token = "MTIz..."
/// allow_guilds = ["123456789"]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DiscordChannelConfig {
    pub bot_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub allow_guilds: Vec<String>,
}

/// HTTP channel configuration.
///
/// ```toml
/// [channel.http]
/// port = 3000
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct HttpChannelConfig {
    pub port: Option<u16>,
}

/// Heartbeat channel configuration.
///
/// ```toml
/// [channel.heartbeat]
/// interval_mins = 30
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct HeartbeatChannelConfig {
    /// Interval in minutes between heartbeats (default: 30).
    pub interval_mins: Option<u64>,
}

/// Cron channel configuration.
///
/// ```toml
/// [channel.cron]
/// tick_interval_secs = 15
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct CronChannelConfig {
    /// How often to check for due jobs (seconds, default: 15).
    pub tick_interval_secs: Option<u64>,
}

/// All channel configurations grouped together.
///
/// ```toml
/// [channel.telegram]
/// bot_token = "..."
///
/// [channel.heartbeat]
/// interval_mins = 15
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ChannelConfigs {
    pub telegram: TelegramChannelConfig,
    pub discord: DiscordChannelConfig,
    pub http: HttpChannelConfig,
    pub heartbeat: HeartbeatChannelConfig,
    pub cron: CronChannelConfig,
}

// ── Main config ──

/// Complete nexal configuration.
///
/// Loaded from `~/.nexal/config.toml` + `NEXAL_*` environment variables.
/// LLM providers are configured under `[providers.<name>]`.
/// Channel-specific settings are under `[channel.<name>]`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NexalConfig {
    /// Override the nexal home directory (default: ~/.nexal)
    pub nexal_home: Option<PathBuf>,

    /// Workspace root directory
    pub workspace: PathBuf,

    /// Path to SOUL.md persona file
    pub soul_path: Option<PathBuf>,

    /// Admin usernames
    #[serde(default, deserialize_with = "deserialize_string_or_int_vec")]
    pub admins: Vec<String>,

    /// Debounce delay after mention (seconds)
    pub debounce_secs: f64,
    /// Delay for follow-up messages in active window (seconds)
    pub message_delay_secs: f64,
    /// Active conversation window (seconds)
    pub active_window_secs: f64,

    /// Sandbox backend: "podman" (default)
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

    /// Jina AI API key (for search and reader)
    pub jina_api_key: Option<String>,

    /// Per-channel configurations
    #[serde(default)]
    pub channel: ChannelConfigs,

    /// LLM provider configurations
    pub providers: HashMap<String, ProviderConfig>,

    /// OpenTelemetry configuration. Omit or leave endpoint empty to disable.
    #[serde(default)]
    pub otel: OtelConfig,

    // ── Backward-compat flat fields (hidden from TOML, populated from env) ──
    // These are kept so old env vars like TELEGRAM_BOT_TOKEN still work.
    // They are merged into channel.* in from_env() post-processing.
    #[serde(default, skip_serializing)]
    pub telegram_bot_token: Option<String>,
    #[serde(default, skip_serializing)]
    pub discord_bot_token: Option<String>,
    #[serde(default, skip_serializing)]
    pub http_channel_port: Option<u16>,
    #[serde(default, skip_serializing)]
    pub heartbeat_interval_mins: Option<u64>,
}

/// OpenTelemetry export configuration.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct OtelConfig {
    pub endpoint: Option<String>,
    pub headers: HashMap<String, String>,
    pub service_name: Option<String>,
}

/// Configuration for a single LLM provider.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub wire_api: Option<String>,
    pub thinking_mode: bool,
    pub http_headers: Option<HashMap<String, String>>,
    pub env_http_headers: Option<HashMap<String, String>>,
    pub query_params: Option<HashMap<String, String>>,
}

impl Default for NexalConfig {
    fn default() -> Self {
        let home = dirs_home();
        Self {
            nexal_home: None,
            workspace: home.join(".nexal").join("workspace"),
            soul_path: None,
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
            jina_api_key: None,
            skills_dir: None,
            channel: ChannelConfigs::default(),
            providers: HashMap::new(),
            otel: OtelConfig::default(),
            // Backward-compat flat fields
            telegram_bot_token: None,
            discord_bot_token: None,
            http_channel_port: None,
            heartbeat_interval_mins: None,
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
                    .map(|key| key.into()),
            );

        // Also merge non-prefixed env vars for backward compatibility
        let figment = figment
            .merge(Env::raw().only(&[
                "TELEGRAM_BOT_TOKEN",
                "DISCORD_BOT_TOKEN",
                "SANDBOX_IMAGE",
                "SANDBOX_RUNTIME",
                "JINA_API_KEY",
            ]).map(|key| {
                match key.as_str() {
                    "TELEGRAM_BOT_TOKEN" => "telegram_bot_token".into(),
                    "DISCORD_BOT_TOKEN" => "discord_bot_token".into(),
                    "SANDBOX_IMAGE" => "sandbox_image".into(),
                    "SANDBOX_RUNTIME" => "sandbox_runtime".into(),
                    "JINA_API_KEY" => "jina_api_key".into(),
                    _ => key.into(),
                }
            }));

        match figment.extract::<NexalConfig>() {
            Ok(mut config) => {
                if config.soul_path.is_none() {
                    config.soul_path = Some(config.workspace.join("agents").join("SOUL.md"));
                }

                // ── Merge backward-compat flat fields into channel.* ──
                if config.channel.telegram.bot_token.is_none() {
                    if let Some(ref token) = config.telegram_bot_token {
                        config.channel.telegram.bot_token = Some(token.clone());
                    }
                }
                if config.channel.discord.bot_token.is_none() {
                    if let Some(ref token) = config.discord_bot_token {
                        config.channel.discord.bot_token = Some(token.clone());
                    }
                }
                if config.channel.http.port.is_none() {
                    config.channel.http.port = config.http_channel_port;
                }
                if config.channel.heartbeat.interval_mins.is_none() {
                    config.channel.heartbeat.interval_mins = config.heartbeat_interval_mins;
                }

                // Normalize comma-separated list fields
                fn normalize_list(v: &[String]) -> Vec<String> {
                    v.iter()
                        .flat_map(|s| s.split(',').map(|a| a.trim().trim_matches('@').to_string()))
                        .filter(|a| !a.is_empty())
                        .collect()
                }
                config.admins = normalize_list(&config.admins);
                config.channel.telegram.allow_from = normalize_list(&config.channel.telegram.allow_from);
                config.channel.telegram.allow_chats = normalize_list(&config.channel.telegram.allow_chats);
                config.channel.discord.allow_guilds = normalize_list(&config.channel.discord.allow_guilds);
                config
            }
            Err(e) => {
                tracing::warn!("config load error (using defaults): {e}");
                Self::default()
            }
        }
    }

    // ── Convenience accessors ──

    pub fn workspace_dir(&self) -> &PathBuf {
        &self.workspace
    }

    pub fn soul_path(&self) -> PathBuf {
        self.soul_path
            .clone()
            .unwrap_or_else(|| self.workspace.join("agents").join("SOUL.md"))
    }

    pub fn sandbox_backend(&self) -> SandboxBackend {
        SandboxBackend::Podman
    }

    pub async fn load_soul(&self) -> String {
        let path = self.soul_path();
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&path, DEFAULT_SOUL).await;
        let base = DEFAULT_SOUL.to_string();

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
        self.channel.telegram.allow_from.is_empty()
            || self.channel.telegram.allow_from.iter().any(|u| u == username)
    }

    pub fn is_telegram_allowed_chat(&self, chat_id: &str) -> bool {
        self.channel.telegram.allow_chats.is_empty()
            || self.channel.telegram.allow_chats.iter().any(|c| c == chat_id)
    }

    pub fn is_discord_allowed_guild(&self, guild_id: &str) -> bool {
        self.channel.discord.allow_guilds.is_empty()
            || self.channel.discord.allow_guilds.iter().any(|g| g == guild_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Podman,
}

impl fmt::Display for SandboxBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Podman => write!(f, "podman"),
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
    fn nested_channel_config_from_toml() {
        use figment::providers::{Format, Serialized, Toml};

        let toml_str = r#"
            [channel.telegram]
            bot_token = "123:ABC"
            allow_from = ["alice"]

            [channel.heartbeat]
            interval_mins = 15

            [channel.http]
            port = 8080
        "#;

        let config: NexalConfig = Figment::new()
            .merge(Serialized::defaults(NexalConfig::default()))
            .merge(Toml::string(toml_str))
            .extract()
            .unwrap();

        assert_eq!(config.channel.telegram.bot_token.as_deref(), Some("123:ABC"));
        assert_eq!(config.channel.telegram.allow_from, vec!["alice"]);
        assert_eq!(config.channel.heartbeat.interval_mins, Some(15));
        assert_eq!(config.channel.http.port, Some(8080));
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
