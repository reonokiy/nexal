from dataclasses import dataclass, field
import json
from datetime import datetime
from typing import Any

from deepresearch.tools.base import FunctionTool, ToolContext, ToolExecutionResult


def _current_datetime_parameters() -> dict[str, Any]:
    return {
        "type": "object",
        "properties": {},
        "additionalProperties": False,
    }


@dataclass(frozen=True)
class CurrentDatetimeTool(FunctionTool):
    name: str = "get_current_datetime"
    description: str = "Get the current local date and time from the system clock."
    parameters: dict[str, Any] = field(default_factory=_current_datetime_parameters, init=False)

    def execute(self, arguments: str, context: ToolContext) -> ToolExecutionResult:
        now = datetime.now().astimezone()
        output = json.dumps(
            {
                "iso_datetime": now.isoformat(),
                "date": now.date().isoformat(),
                "time": now.strftime("%H:%M:%S"),
                "weekday": now.strftime("%A"),
                "timezone": str(now.tzinfo),
            },
            ensure_ascii=False,
        )
        return ToolExecutionResult(output=output, sandbox_session=context.sandbox_session)
