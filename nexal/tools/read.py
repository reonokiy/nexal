"""Read tool — read files from the workspace with text, image, and binary support."""

from __future__ import annotations

import base64
import json
import logging
from dataclasses import dataclass, field
from typing import Any, ClassVar

from nexal.settings import settings
from nexal.tools.base import FunctionTool
from nexal.workspace import resolve_workspace_path, to_container_path

logger = logging.getLogger("nexal.tools.read")

# Extensions treated as images for multimodal models.
_IMAGE_EXTENSIONS: set[str] = {
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp",
}

# MIME types for image extensions.
_IMAGE_MIME: dict[str, str] = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".webp": "image/webp",
    ".bmp": "image/bmp",
}

# Max image size to inline as base64 (10 MB).
_MAX_IMAGE_BYTES: int = 10_000_000

# Default line limit when reading text files.
_DEFAULT_LIMIT: int = 2000


@dataclass
class ReadParams:
    file_path: str
    offset: int = 1
    limit: int = _DEFAULT_LIMIT


@dataclass
class ReadTool(FunctionTool):
    name: str = "read"
    description: str = (
        "Read a file from /workspace. Returns text files with line numbers. "
        "For image files (png, jpg, gif, webp, bmp), returns the image content "
        "directly so you can see it (requires multimodal model support). "
        "Use offset and limit to read specific portions of large files."
    )
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (e.g. /workspace/src/main.py or src/main.py).",
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-based, default 1). Only for text files.",
                    "default": 1,
                    "minimum": 1,
                },
                "limit": {
                    "type": "integer",
                    "description": f"Maximum number of lines to read (default {_DEFAULT_LIMIT}). Only for text files.",
                    "default": _DEFAULT_LIMIT,
                    "minimum": 1,
                },
            },
            "required": ["file_path"],
            "additionalProperties": False,
        },
        init=False,
    )
    params_type: ClassVar[type] = ReadParams

    # Set at runtime: whether the LLM supports image content.
    multimodal: bool = True

    def run_multimodal(self, arguments: str) -> str | list[dict[str, Any]]:
        """Run the read tool with multimodal image support."""
        params, error = self._parse_params(arguments)
        if error is not None:
            return error
        return self.execute_multimodal(params)

    def execute(self, params: ReadParams) -> str:
        """Read text files. Returns string with line numbers."""
        try:
            host_path = resolve_workspace_path(params.file_path)
        except ValueError as e:
            return json.dumps({"error": str(e)})

        if not host_path.exists():
            return json.dumps({"error": f"File not found: {to_container_path(params.file_path)}"})
        if host_path.is_dir():
            return json.dumps({"error": f"Path is a directory, not a file: {to_container_path(params.file_path)}"})

        suffix = host_path.suffix.lower()

        # Image files — if multimodal is off, just return metadata.
        if suffix in _IMAGE_EXTENSIONS:
            if not self.multimodal or not settings.llm_supports_images:
                size = host_path.stat().st_size
                return json.dumps({
                    "file": to_container_path(params.file_path),
                    "type": "image",
                    "mime": _IMAGE_MIME.get(suffix, "application/octet-stream"),
                    "size_bytes": size,
                    "note": "Image content not shown — multimodal support is disabled. Use exec to process this file.",
                })
            # Multimodal path handled by execute_multimodal().
            return self._read_image_fallback(host_path, params.file_path)

        # Text files.
        return self._read_text(host_path, params)

    def execute_multimodal(self, params: ReadParams) -> str | list[dict[str, Any]]:
        """Read files, returning multimodal content for images when supported."""
        try:
            host_path = resolve_workspace_path(params.file_path)
        except ValueError as e:
            return json.dumps({"error": str(e)})

        if not host_path.exists():
            return json.dumps({"error": f"File not found: {to_container_path(params.file_path)}"})
        if host_path.is_dir():
            return json.dumps({"error": f"Path is a directory, not a file: {to_container_path(params.file_path)}"})

        suffix = host_path.suffix.lower()

        # Image files with multimodal support.
        if suffix in _IMAGE_EXTENSIONS and self.multimodal and settings.llm_supports_images:
            return self._read_image_multimodal(host_path, params.file_path, suffix)

        # Everything else goes through the normal text path.
        return self.execute(params)

    def _read_text(self, host_path: Any, params: ReadParams) -> str:
        """Read a text file and return content with line numbers."""
        try:
            raw = host_path.read_text(encoding="utf-8", errors="replace")
        except Exception as e:
            return json.dumps({"error": f"Failed to read file: {e}"})

        lines = raw.splitlines()
        total = len(lines)

        offset = max(1, params.offset)
        limit = max(1, params.limit)
        start = offset - 1  # 0-based index
        end = min(start + limit, total)

        if start >= total:
            return json.dumps({
                "file": to_container_path(params.file_path),
                "total_lines": total,
                "error": f"Offset {offset} exceeds total lines ({total})",
            })

        # Format with line numbers (cat -n style).
        width = len(str(end))
        numbered = []
        for i in range(start, end):
            numbered.append(f"{i + 1:>{width}}\t{lines[i]}")
        content = "\n".join(numbered)

        header_parts = [to_container_path(params.file_path)]
        if offset > 1 or end < total:
            header_parts.append(f"lines {offset}-{end} of {total}")
        else:
            header_parts.append(f"{total} lines")

        return f"[{' | '.join(header_parts)}]\n{content}"

    def _read_image_multimodal(
        self, host_path: Any, file_path: str, suffix: str
    ) -> str | list[dict[str, Any]]:
        """Read an image and return multimodal content blocks."""
        size = host_path.stat().st_size
        if size > _MAX_IMAGE_BYTES:
            return json.dumps({
                "file": to_container_path(file_path),
                "type": "image",
                "size_bytes": size,
                "error": f"Image too large ({size:,} bytes, max {_MAX_IMAGE_BYTES:,}). Use exec to process it.",
            })

        try:
            data = host_path.read_bytes()
        except Exception as e:
            return json.dumps({"error": f"Failed to read image: {e}"})

        mime = _IMAGE_MIME.get(suffix, "image/png")
        b64 = base64.b64encode(data).decode("ascii")
        data_url = f"data:{mime};base64,{b64}"

        return [
            {"type": "text", "text": f"[Image: {to_container_path(file_path)} ({size:,} bytes)]"},
            {"type": "image_url", "image_url": {"url": data_url}},
        ]

    def _read_image_fallback(self, host_path: Any, file_path: str) -> str:
        """Fallback for images when called via execute() instead of execute_multimodal()."""
        size = host_path.stat().st_size
        suffix = host_path.suffix.lower()
        return json.dumps({
            "file": to_container_path(file_path),
            "type": "image",
            "mime": _IMAGE_MIME.get(suffix, "application/octet-stream"),
            "size_bytes": size,
            "note": "Use exec to process this image, or the model will see it via multimodal content.",
        })
