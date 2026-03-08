"""Host-side file operations for the /workspace/agents/ directory.

All writes to /workspace/agents/ go through this module, bypassing the sandbox
exec layer. This keeps the directory read-only from the LLM's perspective inside
the container while allowing system code to manage it directly via the bind mount.
"""

import logging
from pathlib import Path

from nexal.settings import settings

logger = logging.getLogger("nexal.workspace")

_AGENTS_DIR = "agents"


def _host_agents_dir() -> Path:
    """Return the host-side path for /workspace/agents/."""
    workspace_dir = settings.sandbox_workspace_dir
    if not workspace_dir:
        raise RuntimeError("sandbox_workspace_dir is not set")
    return Path(workspace_dir) / _AGENTS_DIR


def _safe_resolve(rel_path: str) -> Path:
    """Resolve rel_path under agents dir, rejecting path traversal."""
    base = _host_agents_dir().resolve()
    resolved = (base / rel_path).resolve()
    if not resolved.is_relative_to(base):
        raise ValueError(f"Path escapes agents directory: {rel_path}")
    return resolved


def read_agents_file(rel_path: str) -> str | None:
    """Read a file under /workspace/agents/<rel_path>. Returns content or None if missing."""
    path = _safe_resolve(rel_path)
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def write_agents_file(rel_path: str, content: str) -> str:
    """Write a file under /workspace/agents/<rel_path>.

    Returns the container-side path (/workspace/agents/<rel_path>).
    """
    host_path = _safe_resolve(rel_path)
    host_path.parent.mkdir(parents=True, exist_ok=True)
    host_path.write_text(content, encoding="utf-8")
    return f"/workspace/{_AGENTS_DIR}/{rel_path}"
