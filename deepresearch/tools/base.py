from dataclasses import dataclass
from typing import Any, Protocol

from deepresearch.sandbox.base import SandboxSession


@dataclass
class ToolContext:
    settings: Any
    sandbox_session: SandboxSession | None = None


@dataclass
class ToolExecutionResult:
    output: str
    sandbox_session: SandboxSession | None = None


class AgentTool(Protocol):
    name: str
    description: str
    parameters: dict[str, Any]

    def to_openai_tool(self) -> dict[str, Any]:
        pass

    def execute(self, arguments: str, context: ToolContext) -> ToolExecutionResult:
        pass


@dataclass(frozen=True)
class FunctionTool:
    name: str
    description: str
    parameters: dict[str, Any]

    def to_openai_tool(self) -> dict[str, Any]:
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        }
