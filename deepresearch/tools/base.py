from dataclasses import dataclass
from typing import Any

from deepresearch.settings import AgentSettings


@dataclass
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

    def execute(self, arguments: str, settings: AgentSettings) -> str:
        raise NotImplementedError

    def close(self) -> None:
        pass
