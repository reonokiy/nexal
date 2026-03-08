from dataclasses import dataclass, field
import json
import logging
from typing import Any, ClassVar

from nexal.sandbox import Sandbox, SandboxConfig, SandboxExecRequest
from nexal.sandbox.base import SandboxSession
from nexal.settings import settings
from nexal.tools.base import FunctionTool


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
        return Sandbox(
            config=SandboxConfig(
                session_id=settings.sandbox_session_id,
                workspace_dir=settings.sandbox_workspace_dir or None,
                workspace_read_only=settings.sandbox_workspace_read_only,
                network=network,
            )
        ).start()
