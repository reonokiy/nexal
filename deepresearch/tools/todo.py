from dataclasses import dataclass, field
import json
from typing import Any, ClassVar

from deepresearch.tools.base import FunctionTool
from deepresearch.workspace import read_agents_file, write_agents_file

DEFAULT_TODO_PATH = "TODO.md"

# content can be str or list[str], but base.py type validation only handles
# simple types. We skip framework validation by using Any and validate manually.


@dataclass
class TodoParams:
    action: str
    content: Any = ""
    index: int = 0


@dataclass
class TodoTool(FunctionTool):
    name: str = "todo"
    description: str = (
        "Manage a TODO list for tracking tasks. "
        "Supports read/add/remove/clear actions. "
        "Stored as a Markdown checklist at /workspace/agents/TODO.md."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "add", "remove", "clear"],
                    "description": "read: list all items; add: append new item(s); remove: remove item by index (1-based); clear: remove all items.",
                },
                "content": {
                    "oneOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}},
                    ],
                    "description": "TODO item(s) to add (required for 'add'). A single string or an array of strings.",
                },
                "index": {
                    "type": "integer",
                    "description": "1-based index of the item to remove (required for 'remove').",
                },
            },
            "required": ["action"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = TodoParams
    path: str = DEFAULT_TODO_PATH

    @staticmethod
    def _parse_item(line: str) -> dict[str, Any]:
        """Parse a markdown checklist line into a structured dict."""
        done = line.startswith("- [x]") or line.startswith("- [X]")
        text = line.split("]", 1)[1].strip() if "]" in line else line
        return {"text": text, "done": done}

    def _read_items(self) -> tuple[list[str], list[dict[str, Any]], bool]:
        """Read TODO items. Returns (raw_lines, parsed_items, was_reset)."""
        text = read_agents_file(self.path)
        if text is None:
            return [], [], False
        lines = text.splitlines()
        raw = [line for line in lines if line.startswith("- [")]
        # If file has content but no valid checklist items, it's corrupted.
        non_empty = [line for line in lines if line.strip() and not line.startswith("#")]
        if non_empty and not raw:
            self._write_items([])
            return [], [], True
        parsed = [self._parse_item(line) for line in raw]
        return raw, parsed, False

    def _write_items(self, items: list[str]) -> None:
        content = ("# TODO\n\n" + "\n".join(items) + "\n") if items else "# TODO\n"
        write_agents_file(self.path, content)

    def _result(self, data: dict[str, Any], was_reset: bool) -> str:
        if was_reset:
            data["warning"] = "TODO file was corrupted or had invalid format and has been reset"
        return json.dumps(data, ensure_ascii=False)

    def execute(self, params: TodoParams) -> str:
        if params.action == "read":
            raw, parsed, was_reset = self._read_items()
            return self._result({"items": parsed, "count": len(parsed)}, was_reset)

        if params.action == "add":
            # Normalize content to list (accept both string and array).
            c = params.content
            if isinstance(c, str):
                items_input = [c]
            elif isinstance(c, list):
                items_input = c
            else:
                return json.dumps({"error": "content must be a string or array of strings"})
            texts = [s.strip().replace("\n", " ").replace("\r", "") for s in items_input if isinstance(s, str) and s.strip()]
            if not texts:
                return json.dumps({"error": "content is required for 'add' action"})
            raw, _, was_reset = self._read_items()
            for text in texts:
                raw.append(f"- [ ] {text}")
            self._write_items(raw)
            return self._result({"add": len(texts), "total": len(raw)}, was_reset)

        if params.action == "remove":
            raw, parsed, was_reset = self._read_items()
            idx = params.index
            if idx == 0:
                return json.dumps({"error": "index is required for 'remove' action (1-based)"})
            if not raw:
                return json.dumps({"error": "TODO list is empty, nothing to remove"})
            if idx < 1 or idx > len(raw):
                return json.dumps({"error": f"Invalid index {idx}, must be 1-{len(raw)}"})
            removed = parsed[idx - 1]
            raw.pop(idx - 1)
            self._write_items(raw)
            return self._result({"removed": removed, "count": len(raw)}, was_reset)

        if params.action == "clear":
            self._write_items([])
            return json.dumps({"cleared": True, "count": 0})

        return json.dumps({"error": f"Unknown action: {params.action}"})
