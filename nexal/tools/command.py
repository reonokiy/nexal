from dataclasses import dataclass, field
import json
import logging
from pathlib import Path
from typing import Any, ClassVar

from nexal.proxy import start_proxies
from nexal.sandbox import Sandbox, SandboxConfig, SandboxExecRequest
from nexal.sandbox.base import SandboxMount, SandboxSession
from nexal.settings import settings
from nexal.tools.base import FunctionTool

_SKILLS_DIR = Path(__file__).resolve().parent.parent / "skills"
_CONTAINER_SKILLS_DIR = "/workspace/agents/skills"


logger = logging.getLogger("nexal.agent")


@dataclass
class ExecParams:
    command: str
    timeout_seconds: int = 60


@dataclass
class ExecTool(FunctionTool):
    name: str = "exec"
    description: str = (
        "Run a command in the persistent sandbox environment. Use /workspace for files you want to keep. "
        "Environment variables (export) and working directory (cd) persist across calls. "
        "Pre-installed tools: python3, uv, pixi, git, curl, wget, jq, ripgrep (rg). "
        "To install Python packages: `uv venv && uv pip install <pkg>` (creates .venv and installs into it). "
        "Activate with `source .venv/bin/activate` or run directly with `.venv/bin/python`. "
        "Do not use bare pip or apt."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute inside the container.",
                },
                "timeout_seconds": {
                    "type": "integer",
                    "default": 60,
                    "minimum": 1,
                    "maximum": 600,
                },
            },
            "required": ["command"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = ExecParams
    _session: SandboxSession | None = field(default=None, init=False, repr=False)
    _proxy_servers: list = field(default_factory=list, init=False, repr=False)

    def execute(self, params: ExecParams) -> str:
        timeout = max(1, min(params.timeout_seconds, 600))
        request = SandboxExecRequest(
            command=params.command,
            timeout_seconds=timeout,
        )
        if self._session is None:
            self._session = self._start_sandbox()
        result = self._session.exec(request)
        return json.dumps({
            "exit_code": result.exit_code,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }, ensure_ascii=False)

    def close(self) -> None:
        for srv in self._proxy_servers:
            try:
                srv.shutdown()
            except Exception:
                pass
        self._proxy_servers = []
        if self._session is None:
            return
        try:
            result = self._session.stop()
            logger.info(
                "sandbox_session_stopped session_id=%s exit_code=%s",
                result.session_id, result.exit_code,
            )
        except KeyboardInterrupt:
            logger.warning("sandbox_session_stop_interrupted")
        except Exception:
            logger.exception("sandbox_session_stop_failed")
        finally:
            self._session = None

    def _start_sandbox(self) -> SandboxSession:
        network = "pasta" if settings.sandbox_network_enabled else "none"
        shared_dirs: list[SandboxMount] = []
        if _SKILLS_DIR.is_dir():
            shared_dirs.append(SandboxMount(
                host_path=str(_SKILLS_DIR),
                container_path=_CONTAINER_SKILLS_DIR,
                read_only=True,
            ))
        # Start token proxies — tokens stay on the host, only the sockets
        # are accessible inside the container via the workspace bind mount.
        env: dict[str, str] = {}
        if settings.sandbox_workspace_dir and (
            settings.telegram_bot_token or settings.discord_bot_token
        ):
            self._proxy_servers = start_proxies(
                workspace_dir=settings.sandbox_workspace_dir,
                telegram_token=settings.telegram_bot_token or None,
                discord_token=settings.discord_bot_token or None,
            )

        return Sandbox(
            config=SandboxConfig(
                session_id=settings.sandbox_session_id,
                workspace_dir=settings.sandbox_workspace_dir or None,
                workspace_read_only=settings.sandbox_workspace_read_only,
                shared_dirs=shared_dirs,
                env=env,
                runtime=settings.sandbox_runtime or None,
                network=network,
            )
        ).start()
