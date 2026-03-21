from dataclasses import dataclass
import logging
import os
from pathlib import Path
from uuid6 import uuid7


logger = logging.getLogger("nexal.agent")


@dataclass
class AgentSettings:
    llm_api_base: str = ""
    llm_api_key: str = ""
    llm_model: str = ""
    sandbox_session_id: str = ""
    sandbox_workspace_read_only: bool = False
    sandbox_workspace_dir: str = ""
    sandbox_network_enabled: bool = False
    llm_max_context_tokens: int = 128_000
    llm_temperature: float = 0.6
    llm_top_p: float = 0.95
    llm_supports_images: bool = True
    telegram_bot_token: str = ""
    telegram_allow_from: list[str] | None = None
    telegram_allow_chats: list[str] | None = None
    discord_bot_token: str = ""
    discord_allow_from: list[str] | None = None
    discord_allow_channels: list[str] | None = None
    bot_name: str = "nexal"
    message_debounce_seconds: int = 1
    message_delay_seconds: int = 10
    active_time_window_seconds: int = 60


settings = AgentSettings()
_settings_loaded = False


def load_settings() -> None:
    global _settings_loaded
    api_base = os.getenv("LLM_API_BASE", "")
    model = os.getenv("LLM_MODEL", "openai/gpt-4o")
    api_key = os.getenv("LLM_API_KEY", "")
    sandbox_session_id = os.getenv("SANDBOX_SESSION_ID", "").strip()
    workspace_read_only_env = os.getenv("SANDBOX_WORKSPACE_READ_ONLY", "").strip().lower()
    sandbox_network_env = os.getenv("SANDBOX_NETWORK_ENABLED", "").strip().lower()

    settings.llm_api_base = api_base
    settings.llm_api_key = api_key
    settings.llm_model = model
    settings.sandbox_session_id = sandbox_session_id
    settings.sandbox_workspace_read_only = workspace_read_only_env in {"1", "true", "yes", "on"}
    settings.sandbox_network_enabled = sandbox_network_env in {"1", "true", "yes", "on"}
    settings.llm_max_context_tokens = int(os.getenv("LLM_MAX_CONTEXT_TOKENS", "128000"))
    settings.llm_temperature = float(os.getenv("LLM_TEMPERATURE", "0.6"))
    settings.llm_top_p = float(os.getenv("LLM_TOP_P", "0.95"))
    llm_images_env = os.getenv("LLM_SUPPORTS_IMAGES", "").strip().lower()
    settings.llm_supports_images = llm_images_env not in {"0", "false", "no", "off"}
    settings.telegram_bot_token = os.getenv("TELEGRAM_BOT_TOKEN", "")
    settings.telegram_allow_from = _parse_list_env("TELEGRAM_ALLOW_FROM")
    settings.telegram_allow_chats = _parse_list_env("TELEGRAM_ALLOW_CHATS")
    settings.discord_bot_token = os.getenv("DISCORD_BOT_TOKEN", "")
    settings.discord_allow_from = _parse_list_env("DISCORD_ALLOW_FROM")
    settings.discord_allow_channels = _parse_list_env("DISCORD_ALLOW_CHANNELS")
    settings.bot_name = os.getenv("BOT_NAME", "nexal")
    settings.message_debounce_seconds = int(os.getenv("MESSAGE_DEBOUNCE_SECONDS", "1"))
    settings.message_delay_seconds = int(os.getenv("MESSAGE_DELAY_SECONDS", "10"))
    settings.active_time_window_seconds = int(os.getenv("ACTIVE_TIME_WINDOW_SECONDS", "60"))
    _settings_loaded = True


def _parse_list_env(key: str) -> list[str] | None:
    val = os.getenv(key, "").strip()
    if not val:
        return None
    return [item.strip() for item in val.split(",") if item.strip()]


def llm_kwargs() -> dict:
    """Common kwargs for litellm.completion calls."""
    kwargs: dict = {"model": settings.llm_model, "timeout": 300.0}
    if settings.llm_api_key:
        kwargs["api_key"] = settings.llm_api_key
    if settings.llm_api_base:
        kwargs["api_base"] = settings.llm_api_base
    return kwargs


def ensure_sandbox_session() -> None:
    if not _settings_loaded:
        raise RuntimeError("load_settings() must be called before using the agent")
    if settings.sandbox_workspace_dir:
        return

    default_root = str(Path.home().joinpath(".nexal", "sessions"))
    root = Path(os.getenv("SANDBOX_SESSIONS_DIR", default_root))
    session_name = settings.sandbox_session_id or str(uuid7())
    workspace_dir = root / session_name
    workspace_dir.mkdir(parents=True, exist_ok=True)

    settings.sandbox_session_id = session_name
    settings.sandbox_workspace_dir = str(workspace_dir)
    logger.info("sandbox_session_ready session_id=%s workspace_dir=%s", session_name, workspace_dir)
