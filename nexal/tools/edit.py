"""Edit tool — exact string replacement in workspace files."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, ClassVar

from nexal.tools.base import FunctionTool
from nexal.workspace import resolve_workspace_path, to_container_path

_DIFF_CONTEXT_LINES = 4


@dataclass
class EditParams:
    file_path: str
    old_string: str
    new_string: str
    replace_all: bool = False


@dataclass
class EditTool(FunctionTool):
    name: str = "edit"
    description: str = (
        "Perform exact string replacements in a file under /workspace. "
        "Provide old_string (the text to find) and new_string (the replacement). "
        "The edit will FAIL if old_string is not found or is not unique in the file — "
        "provide more surrounding context to make it unique, or set replace_all to true "
        "to replace every occurrence. new_string must differ from old_string."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (e.g. /workspace/src/main.py).",
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find and replace. Must match the file content exactly (including whitespace and indentation).",
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text. Must be different from old_string.",
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "If true, replace all occurrences of old_string. Default false (requires unique match).",
                    "default": False,
                },
            },
            "required": ["file_path", "old_string", "new_string"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = EditParams

    def execute(self, params: EditParams) -> str:
        try:
            host_path = resolve_workspace_path(params.file_path)
        except ValueError as e:
            return json.dumps({"error": str(e)})

        if not host_path.exists():
            return json.dumps({"error": f"File not found: {to_container_path(params.file_path)}"})
        if host_path.is_dir():
            return json.dumps({"error": f"Path is a directory: {to_container_path(params.file_path)}"})

        if params.old_string == params.new_string:
            return json.dumps({"error": "old_string and new_string are identical — no change needed"})

        try:
            content = host_path.read_text(encoding="utf-8")
        except Exception as e:
            return json.dumps({"error": f"Failed to read file: {e}"})

        count = content.count(params.old_string)
        if count == 0:
            return json.dumps({"error": "old_string not found in file. Make sure it matches the file content exactly, including whitespace and indentation."})

        if count > 1 and not params.replace_all:
            return json.dumps({
                "error": f"old_string found {count} times in file. Provide more surrounding context to make it unique, or set replace_all to true.",
                "occurrences": count,
            })

        if params.replace_all:
            new_content = content.replace(params.old_string, params.new_string)
        else:
            new_content = content.replace(params.old_string, params.new_string, 1)

        try:
            host_path.write_text(new_content, encoding="utf-8")
        except Exception as e:
            return json.dumps({"error": f"Failed to write file: {e}"})

        # Build a concise diff preview.
        replaced = count if params.replace_all else 1
        container_path = to_container_path(params.file_path)

        # Show the region around the first replacement.
        lines = new_content.splitlines()
        # Find the line containing the start of new_string.
        first_new_pos = content.find(params.old_string)
        line_no = content[:first_new_pos].count("\n") + 1
        start = max(0, line_no - 1 - _DIFF_CONTEXT_LINES)
        end = min(len(lines), line_no + params.new_string.count("\n") + _DIFF_CONTEXT_LINES)
        width = len(str(end))
        snippet = "\n".join(f"{i + 1:>{width}}\t{lines[i]}" for i in range(start, end))

        return (
            f"Replaced {replaced} occurrence(s) in {container_path}\n\n"
            f"[{container_path} | lines {start + 1}-{end}]\n{snippet}"
        )
