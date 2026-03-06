from dataclasses import dataclass
import logging
import os
from pathlib import Path
from uuid import uuid4


logger = logging.getLogger("deepresearch.agent")


@dataclass
class AgentSettings:
    llm_api_endpoint: str
    llm_api_key: str
    llm_model: str
    sandbox_session_id: str = ""
    sandbox_workspace_read_only: bool = False
    sandbox_workspace_dir: str = ""
    sandbox_network_enabled: bool = True


def load_settings() -> AgentSettings:
    endpoint = os.getenv("LLM_ENDPOINT", "https://openrouter.ai/api/v1")
    model = os.getenv("LLM_MODEL", "openai/gpt-4o")
    api_key = os.getenv("LLM_API_KEY")
    sandbox_session_id = os.getenv("SANDBOX_SESSION_ID", "").strip()
    workspace_read_only_env = os.getenv("SANDBOX_WORKSPACE_READ_ONLY", "").strip().lower()
    sandbox_network_env = os.getenv("SANDBOX_NETWORK_ENABLED", "").strip().lower()
    if not api_key:
        raise RuntimeError("LLM_API_KEY environment variable is required")

    return AgentSettings(
        llm_api_endpoint=endpoint,
        llm_api_key=api_key,
        llm_model=model,
        sandbox_session_id=sandbox_session_id,
        sandbox_workspace_read_only=workspace_read_only_env in {"1", "true", "yes", "on"},
        sandbox_network_enabled=sandbox_network_env not in {"0", "false", "no", "off"},
    )


def ensure_sandbox_session(settings: AgentSettings) -> AgentSettings:
    if settings.sandbox_workspace_dir:
        return settings

    root = Path(os.getenv("SANDBOX_SESSIONS_DIR", ".sandbox_sessions")).resolve()
    root.mkdir(parents=True, exist_ok=True)
    session_name = settings.sandbox_session_id or uuid4().hex
    workspace_dir = root / session_name
    workspace_dir.mkdir(parents=True, exist_ok=True)

    settings.sandbox_session_id = session_name
    settings.sandbox_workspace_dir = str(workspace_dir)
    logger.info("sandbox_session_ready session_id=%s workspace_dir=%s", session_name, workspace_dir)
    return settings
