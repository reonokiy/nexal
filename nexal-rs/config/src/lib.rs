use std::fmt;
use std::path::PathBuf;

const DEFAULT_SOUL: &str = r#"You are Nexal, a helpful AI assistant. You have access to a variety of tools to help you complete tasks.

Guidelines:
- Be concise and helpful
- Use tools when needed to accomplish tasks
- For long responses, break them into multiple shorter messages separated by blank lines
- Always respond in the language the user writes in
"#;

/// Nexal configuration loaded from environment variables.
///
/// These are the nexal-specific settings (channels, sandbox, debounce).
/// LLM model/provider config is handled by `~/.nexal/config.toml`
/// (the forked codex config system), supporting OpenAI, Ollama, LMStudio,
/// and any OpenAI-compatible endpoint via custom `[model_providers.*]`.
#[derive(Debug, Clone)]
pub struct NexalConfig {
    /// Override the nexal config home (default: ~/.nexal)
    pub nexal_home: Option<PathBuf>,

    /// Workspace root directory (default: ~/.nexal/workspace)
    pub workspace_dir: PathBuf,

    /// Path to SOUL.md persona file (default: <workspace>/agents/SOUL.md)
    pub soul_path: PathBuf,

    /// Telegram bot token
    pub telegram_bot_token: Option<String>,
    /// Comma-separated list of allowed Telegram usernames (empty = allow all)
    pub telegram_allow_from: Vec<String>,
    /// Comma-separated list of allowed Telegram chat IDs (empty = allow all)
    pub telegram_allow_chats: Vec<String>,

    /// Discord bot token
    pub discord_bot_token: Option<String>,
    /// Comma-separated list of allowed Discord guild IDs (empty = allow all)
    pub discord_allow_guilds: Vec<String>,

    /// Message debounce delay after mention in seconds (default: 1.0)
    pub debounce_secs: f64,

    /// Delay for follow-up messages in active window in seconds (default: 10.0)
    pub message_delay_secs: f64,

    /// Active conversation window after mention in seconds (default: 60.0)
    pub active_window_secs: f64,

    /// Enable network access inside sandbox (default: false)
    pub sandbox_network: bool,

    /// Sandbox backend: podman (default), bwrap, none
    pub sandbox_backend: SandboxBackend,

    /// Podman container image (default: ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13)
    pub sandbox_image: String,

    /// OCI runtime override for podman (e.g. crun, kata)
    pub sandbox_runtime: Option<String>,

    /// Memory limit for sandbox containers (default: 512m)
    pub sandbox_memory: String,

    /// CPU limit for sandbox containers (default: 1.0)
    pub sandbox_cpus: String,

    /// PID limit for sandbox containers (default: 256)
    pub sandbox_pids_limit: u32,

    /// Path to nexal skills directory (default: <workspace>/skills)
    pub skills_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    /// Run commands inside a Podman container (default)
    Podman,
    /// No sandbox — full host access (not recommended)
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

impl NexalConfig {
    pub fn from_env() -> Self {
        let home = dirs_home();
        let workspace_dir = std::env::var("NEXAL_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".nexal").join("workspace"));

        let soul_path = std::env::var("NEXAL_SOUL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| workspace_dir.join("agents").join("SOUL.md"));

        let nexal_home = std::env::var("NEXAL_HOME").ok().map(PathBuf::from);

        let telegram_bot_token = std::env::var("TELEGRAM_BOT_TOKEN").ok();
        let telegram_allow_from = parse_list("TELEGRAM_ALLOW_FROM");
        let telegram_allow_chats = parse_list("TELEGRAM_ALLOW_CHATS");

        let discord_bot_token = std::env::var("DISCORD_BOT_TOKEN").ok();
        let discord_allow_guilds = parse_list("DISCORD_ALLOW_GUILDS");

        let debounce_secs = std::env::var("NEXAL_DEBOUNCE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);

        let message_delay_secs = std::env::var("NEXAL_MESSAGE_DELAY_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);

        let active_window_secs = std::env::var("NEXAL_ACTIVE_WINDOW_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60.0);

        let sandbox_network = std::env::var("SANDBOX_NETWORK_ENABLED")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        let sandbox_backend = match std::env::var("NEXAL_SANDBOX")
            .unwrap_or_else(|_| "podman".to_string())
            .to_lowercase()
            .as_str()
        {
            "none" | "off" | "disabled" => SandboxBackend::None,
            _ => SandboxBackend::Podman,
        };

        let sandbox_image = std::env::var("SANDBOX_IMAGE")
            .unwrap_or_else(|_| "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13".to_string());
        let sandbox_runtime = std::env::var("SANDBOX_RUNTIME").ok();
        let sandbox_memory = std::env::var("SANDBOX_MEMORY").unwrap_or_else(|_| "512m".to_string());
        let sandbox_cpus = std::env::var("SANDBOX_CPUS").unwrap_or_else(|_| "1.0".to_string());
        let sandbox_pids_limit = std::env::var("SANDBOX_PIDS_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(256);

        let skills_dir = std::env::var("NEXAL_SKILLS_DIR").ok().map(PathBuf::from);

        Self {
            nexal_home,
            workspace_dir,
            soul_path,
            telegram_bot_token,
            telegram_allow_from,
            telegram_allow_chats,
            discord_bot_token,
            discord_allow_guilds,
            debounce_secs,
            message_delay_secs,
            active_window_secs,
            sandbox_network,
            sandbox_backend,
            sandbox_image,
            sandbox_runtime,
            sandbox_memory,
            sandbox_cpus,
            sandbox_pids_limit,
            skills_dir,
        }
    }

    /// Read SOUL.md content. Creates the file with defaults if absent.
    pub async fn load_soul(&self) -> String {
        match tokio::fs::read_to_string(&self.soul_path).await {
            Ok(content) if !content.trim().is_empty() => content,
            _ => {
                // Write default SOUL.md if missing
                if let Some(parent) = self.soul_path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&self.soul_path, DEFAULT_SOUL).await;
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

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn parse_list(env_var: &str) -> Vec<String> {
    std::env::var(env_var)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
