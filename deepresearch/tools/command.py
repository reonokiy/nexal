from dataclasses import dataclass, field
import json
import logging
from typing import Any, ClassVar

from deepresearch.sandbox import Sandbox, SandboxConfig, SandboxExecRequest
from deepresearch.sandbox.base import SandboxSession
from deepresearch.settings import settings
from deepresearch.tools.base import FunctionTool


logger = logging.getLogger("deepresearch.agent")


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
        "You have root access and can install packages with apt-get or pip as needed."
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
        except Exception:
            logger.exception("sandbox_session_stop_failed")
        finally:
            self._session = None

    def _start_sandbox(self) -> SandboxSession:
        network = "bridge" if settings.sandbox_network_enabled else "none"
        return Sandbox(
            config=SandboxConfig(
                session_id=settings.sandbox_session_id,
                workspace_dir=settings.sandbox_workspace_dir or None,
                workspace_read_only=settings.sandbox_workspace_read_only,
                network=network,
            )
        ).start()
