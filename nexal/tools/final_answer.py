from dataclasses import dataclass, field
from typing import Any, ClassVar

from nexal.tools.base import FunctionTool


@dataclass
class FinalAnswerParams:
    answer: str


@dataclass
class FinalAnswerTool(FunctionTool):
    name: str = "final_answer"
    description: str = (
        "Submit your final answer to the user. Only call this when you have thoroughly "
        "investigated the task and are ready to provide a comprehensive response."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string",
                    "description": "Your complete final answer with evidence and sources.",
                },
            },
            "required": ["answer"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = FinalAnswerParams

    def execute(self, params: FinalAnswerParams) -> str:
        return params.answer
