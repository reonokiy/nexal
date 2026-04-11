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
//!
//! After `NexalConfig::from_env()` returns, the value is a **frozen snapshot**
//! of the file + env inputs. The application shares it via `Arc<NexalConfig>`
//! and must not mutate it. Values that have computed defaults (e.g. `soul_path`,
//! `database_url`) are derived lazily through accessor methods.

pub mod sandbox;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

// ── Main config ──

/// Complete nexal configuration.
///
/// Loaded from `~/.nexal/config.toml` + `NEXAL_*` environment variables via
/// [`NexalConfig::from_env`]. After that function returns the value is a frozen
/// snapshot — the application holds it behind `Arc<NexalConfig>` and never
/// mutates it. Accessor methods (e.g. [`soul_path`](Self::soul_path),
/// [`database_url`](Self::database_url)) derive computed defaults lazily.
///
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

    /// Per-channel configurations (raw TOML tables keyed by channel name).
    /// Use each channel crate's `::from_nexal_config()` to get a typed view.
    #[serde(default)]
    pub channel: HashMap<String, toml::Value>,

    /// LLM provider configurations
    pub providers: HashMap<String, ProviderConfig>,

    /// OpenTelemetry configuration. Omit or leave endpoint empty to disable.
    #[serde(default)]
    pub otel: OtelConfig,

    /// Database connection URL.
    ///
    /// Supported schemes: `sqlite://` (default) and `postgres://` /
    /// `postgresql://`. When omitted the database is opened as SQLite inside
    /// `<workspace>/agents/nexal.db`.
    ///
    /// Example: `database_url = "postgres://user:pass@localhost/nexal"`
    pub database_url: Option<String>,

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
            channel: HashMap::new(),
            providers: HashMap::new(),
            otel: OtelConfig::default(),
            database_url: None,
            // Backward-compat flat fields
            telegram_bot_token: None,
            discord_bot_token: None,
            http_channel_port: None,
            heartbeat_interval_mins: None,
        }
    }
}

impl NexalConfig {
    /// Load config from defaults → TOML → env vars and return a frozen snapshot.
    ///
    /// The returned value must not be mutated after this call. Share it via
    /// `Arc<NexalConfig>`. Computed defaults (soul path, database URL) are
    /// resolved lazily by their respective accessor methods.
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

        figment.extract::<NexalConfig>().unwrap_or_else(|e| {
            tracing::warn!("config load error (using defaults): {e}");
            Self::default()
        })
    }

    // ── Convenience accessors ──

    pub fn workspace_dir(&self) -> &Path {
        &self.workspace
    }

    pub fn soul_path(&self) -> PathBuf {
        self.soul_path
            .clone()
            .unwrap_or_else(|| self.workspace.join("agents").join("SOUL.md"))
    }

    /// Returns the database connection URL.
    ///
    /// If `database_url` is set in config/env it is returned verbatim.
    /// Otherwise defaults to a SQLite file at `<workspace>/agents/nexal.db`.
    pub fn database_url(&self) -> String {
        self.database_url.clone().unwrap_or_else(|| {
            let path = self.workspace.join("agents").join("nexal.db");
            format!("sqlite://{}", path.display())
        })
    }

    pub fn sandbox_backend(&self) -> &'static str {
        "podman"
    }

    /// Load the user's persona ("SOUL"). If `SOUL.md` does not yet exist it
    /// is seeded with [`DEFAULT_SOUL`]; if the user has edited it afterwards
    /// their edits are preserved. An optional `SOUL.override.md` alongside it
    /// is appended after a separator.
    pub async fn load_soul(&self) -> String {
        let path = self.soul_path();
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        // Seed the default only when the file is missing — never clobber edits.
        let base = match tokio::fs::read_to_string(&path).await {
            Ok(existing) => existing,
            Err(_) => {
                let _ = tokio::fs::write(&path, DEFAULT_SOUL).await;
                DEFAULT_SOUL.to_string()
            }
        };

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
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Deserialize a `Vec<String>` that may arrive as integers or a
/// comma-separated string (e.g. Telegram chat IDs configured as bare numbers).
///
/// Also normalises each entry: splits on `,`, strips surrounding whitespace,
/// and removes a leading `@` so that `@alice` and `alice` are treated the same.
/// Empty entries after splitting are dropped.
fn deserialize_string_or_int_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};

    fn normalize_entry(s: &str) -> impl Iterator<Item = String> + '_ {
        s.split(',')
            .map(|a| a.trim().trim_start_matches('@').to_string())
            .filter(|a| !a.is_empty())
    }

    struct StringOrIntVec;
    impl<'de> Visitor<'de> for StringOrIntVec {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "a sequence of strings or integers")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut out = Vec::new();
            while let Some(el) = seq.next_element::<serde_json::Value>()? {
                match el {
                    serde_json::Value::String(s) => out.extend(normalize_entry(&s)),
                    serde_json::Value::Number(n) => out.extend(normalize_entry(&n.to_string())),
                    other => out.extend(normalize_entry(&other.to_string())),
                }
            }
            Ok(out)
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Vec<String>, E> {
            Ok(normalize_entry(v).collect())
        }
    }
    deserializer.deserialize_any(StringOrIntVec)
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
        assert_eq!(cfg.sandbox_backend(), "podman");
    }

    #[test]
    fn channel_raw_values_round_trip() {
        let toml_str = r#"
            [channel.telegram]
            bot_token = "123:ABC"

            [channel.heartbeat]
            interval_mins = 15
        "#;

        let config: NexalConfig = Figment::new()
            .merge(Serialized::defaults(NexalConfig::default()))
            .merge(Toml::string(toml_str))
            .extract()
            .unwrap();

        // Channel values are stored as raw toml::Value tables.
        assert!(config.channel.contains_key("telegram"));
        assert!(config.channel.contains_key("heartbeat"));
    }
}
