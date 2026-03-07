from dataclasses import asdict, dataclass, field, fields
import json
import logging
from typing import Any

from deepresearch.sandbox import Sandbox, SandboxConfig, SandboxExecRequest
from deepresearch.sandbox.base import SandboxSession
from deepresearch.settings import AgentSettings
from deepresearch.tools.base import FunctionTool


logger = logging.getLogger("deepresearch.agent")


@dataclass
class RunCommandTool(FunctionTool):
    name: str = "run_command"
    description: str = "Run a command in the persistent working environment. Use /workspace for files you want to keep."
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "command": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command and arguments to execute inside the container.",
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory. Prefer paths under /workspace.",
                    "default": "/workspace",
                },
                "env": {
                    "type": "object",
                    "description": "Environment variables to pass into the container.",
                    "additionalProperties": {"type": "string"},
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
    _session: SandboxSession | None = field(default=None, init=False, repr=False)

    def execute(self, arguments: str, settings: AgentSettings) -> str:
        if self._session is None:
            self._session = self._start_sandbox(settings)
        parsed = json.loads(arguments or "{}")
        valid = {f.name for f in fields(SandboxExecRequest)}
        request = SandboxExecRequest(**{k: v for k, v in parsed.items() if k in valid})
        return json.dumps(asdict(self._session.exec(request)), ensure_ascii=False)

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

    def _start_sandbox(self, settings: AgentSettings) -> SandboxSession:
        network = "host" if settings.sandbox_network_enabled else "none"
        return Sandbox(
            config=SandboxConfig(
                session_id=settings.sandbox_session_id,
                workspace_dir=settings.sandbox_workspace_dir or None,
                workspace_read_only=settings.sandbox_workspace_read_only,
                network=network,
            )
        ).start()
