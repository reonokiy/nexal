from dataclasses import asdict, dataclass, field
import json
from typing import Any

from deepresearch.sandbox import Sandbox, SandboxConfig, SandboxExecRequest
from deepresearch.sandbox.base import SandboxSession
from deepresearch.tools.base import FunctionTool, ToolContext, ToolExecutionResult


def _run_command_parameters() -> dict[str, Any]:
    return {
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
    }


@dataclass(frozen=True)
class RunCommandTool(FunctionTool):
    name: str = "run_command"
    description: str = "Run a command in the persistent working environment. Use /workspace for files you want to keep."
    parameters: dict[str, Any] = field(default_factory=_run_command_parameters, init=False)

    def execute(self, arguments: str, context: ToolContext) -> ToolExecutionResult:
        parsed_args = json.loads(arguments or "{}")
        sandbox_session = context.sandbox_session or self._start_sandbox_session(context.settings)
        request = SandboxExecRequest(
            command=[str(part) for part in parsed_args["command"]],
            workdir=str(parsed_args.get("workdir", "/workspace")),
            env={str(k): str(v) for k, v in parsed_args.get("env", {}).items()},
            timeout_seconds=int(parsed_args.get("timeout_seconds", 60)),
        )
        output = json.dumps(asdict(sandbox_session.exec(request)), ensure_ascii=False)
        return ToolExecutionResult(output=output, sandbox_session=sandbox_session)

    def _start_sandbox_session(self, settings: Any) -> SandboxSession:
        default_network = "host" if settings.sandbox_network_enabled else "none"
        sandbox = Sandbox(
            config=SandboxConfig(
                session_id=settings.sandbox_session_id,
                workspace_dir=settings.sandbox_workspace_dir or None,
                workspace_read_only=bool(settings.sandbox_workspace_read_only),
                network=default_network,
                shared_dirs=[],
            )
        )
        return sandbox.start()
