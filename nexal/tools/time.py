from dataclasses import dataclass, field
import json
from datetime import datetime
from typing import Any

from nexal.tools.base import FunctionTool


@dataclass
class TimeTool(FunctionTool):
    name: str = "time"
    description: str = "Get the current local date and time from the system clock."
    parameters: dict[str, Any] = field(
        default_factory=lambda: {"type": "object", "properties": {}, "additionalProperties": False},
        init=False,
    )

    def execute(self, params: None) -> str:
        now = datetime.now().astimezone()
        return json.dumps(
            {
                "iso_datetime": now.isoformat(),
                "date": now.date().isoformat(),
                "time": now.strftime("%H:%M:%S"),
                "weekday": now.strftime("%A"),
                "timezone": str(now.tzinfo),
            },
            ensure_ascii=False,
        )
