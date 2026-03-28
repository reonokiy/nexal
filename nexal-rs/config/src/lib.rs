//! Nexal configuration — loaded from TOML + environment variables via figment.
//!
//! Config sources (lowest → highest priority):
//! 1. Built-in defaults
//! 2. `~/.nexal/config.toml`
//! 3. Environment variables prefixed with `NEXAL_`
//!
//! Environment variable mapping uses `__` for nesting:
//!   NEXAL_PROVIDERS__MOONSHOT__BASE_URL=https://api.moonshot.cn/v1
//!   NEXAL_SANDBOX=podman
//!   NEXAL_DEBOUNCE_SECS=2.0

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
    /// Comma-separated list of allowed Telegram usernames
    pub telegram_allow_from: Vec<String>,
    /// Comma-separated list of allowed Telegram chat IDs
    pub telegram_allow_chats: Vec<String>,

    /// Discord bot token
    pub discord_bot_token: Option<String>,
    /// Comma-separated list of allowed Discord guild IDs
    pub discord_allow_guilds: Vec<String>,

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
            debounce_secs: 1.0,
            message_delay_secs: 10.0,
            active_window_secs: 60.0,
            sandbox: "podman".to_string(),
            sandbox_image: "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13".to_string(),
            sandbox_runtime: None,
            sandbox_memory: "512m".to_string(),
            sandbox_cpus: "1.0".to_string(),
            sandbox_pids_limit: 256,
            sandbox_network: false,
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
                "LLM_BASE_URL",
                "LLM_API_KEY",
                "LLM_MODEL",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
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
                // Resolve soul_path default
                if config.soul_path.is_none() {
                    config.soul_path = Some(config.workspace.join("agents").join("SOUL.md"));
                }
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

    /// Read SOUL.md content. Creates the file with defaults if absent.
    pub async fn load_soul(&self) -> String {
        let path = self.soul_path();
        match tokio::fs::read_to_string(&path).await {
            Ok(content) if !content.trim().is_empty() => content,
            _ => {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&path, DEFAULT_SOUL).await;
                DEFAULT_SOUL.to_string()
            }
        }
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

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}
