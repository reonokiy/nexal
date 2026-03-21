"""Write tool — create or overwrite files in the workspace."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, ClassVar

from nexal.tools.base import FunctionTool
from nexal.workspace import resolve_workspace_path, to_container_path


@dataclass
class WriteParams:
    file_path: str
    content: str


@dataclass
class WriteTool(FunctionTool):
    name: str = "write"
    description: str = (
        "Create a new file or completely overwrite an existing file under /workspace. "
        "For modifying existing files prefer the edit tool — it only sends the changed region. "
        "Use this tool for creating new files or when the entire content needs to be replaced."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (e.g. /workspace/src/main.py).",
                },
                "content": {
                    "type": "string",
                    "description": "The full content to write to the file.",
                },
            },
            "required": ["file_path", "content"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = WriteParams

    def execute(self, params: WriteParams) -> str:
        try:
            host_path = resolve_workspace_path(params.file_path)
        except ValueError as e:
            return json.dumps({"error": str(e)})

        container_path = to_container_path(params.file_path)
        created = not host_path.exists()

        try:
            host_path.parent.mkdir(parents=True, exist_ok=True)
            host_path.write_text(params.content, encoding="utf-8")
        except Exception as e:
            return json.dumps({"error": f"Failed to write file: {e}"})

        lines = params.content.count("\n") + (1 if params.content else 0)
        action = "Created" if created else "Wrote"
        return f"{action} {container_path} ({lines} lines, {len(params.content)} bytes)"
