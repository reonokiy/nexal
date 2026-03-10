"""Host-side file operations for the /workspace/agents/ directory.

All writes to /workspace/agents/ go through this module, bypassing the sandbox
exec layer. This keeps the directory read-only from the LLM's perspective inside
the container while allowing system code to manage it directly via the bind mount.
"""

import json
import logging
from datetime import datetime, timezone
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


def write_agents_file_bytes(rel_path: str, data: bytes) -> str:
    """Write binary data under /workspace/agents/<rel_path>.

    Returns the container-side path (/workspace/agents/<rel_path>).
    """
    host_path = _safe_resolve(rel_path)
    host_path.parent.mkdir(parents=True, exist_ok=True)
    host_path.write_bytes(data)
    return f"/workspace/{_AGENTS_DIR}/{rel_path}"


# ── Chat history ─────────────────────────────────────────────────


def _conversation_log_path() -> Path:
    """Return path to today's conversations.jsonl: history/YYYY/MM/DD/conversations.jsonl."""
    now = datetime.now(timezone.utc)
    return _safe_resolve(f"history/{now:%Y}/{now:%m}/{now:%d}/conversations.jsonl")


def save_chat_entry(
    channel: str,
    chat_id: str,
    sender: str,
    text: str,
    role: str,
) -> None:
    """Append a chat message to history/YYYY/MM/DD/conversations.jsonl."""
    now = datetime.now(timezone.utc)
    entry = {
        "channel": channel,
        "chat_id": chat_id,
        "sender": sender,
        "text": text,
        "role": role,
        "timestamp": now.isoformat(),
    }
    path = _conversation_log_path()
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry, ensure_ascii=False) + "\n")
    except OSError:
        logger.warning("failed to save chat entry", exc_info=True)


def load_chat_context(limit: int = 50) -> str:
    """Load recent chat entries from history/*/conversations.jsonl files."""
    try:
        history_dir = _host_agents_dir() / "history"
        if not history_dir.exists():
            return "(no conversation history yet)"

        log_files = sorted(history_dir.rglob("conversations.jsonl"))
        if not log_files:
            return "(no conversation history yet)"

        # Take only recent files to avoid scanning entire history.
        log_files = log_files[-30:]

        # Collect entries from recent days, then take the last `limit`.
        all_lines: list[str] = []
        for f in log_files:
            try:
                all_lines.extend(f.read_text(encoding="utf-8").strip().splitlines())
            except OSError:
                continue

        entries: list[dict] = []
        for line in all_lines[-limit:]:
            if not line.strip():
                continue
            try:
                entries.append(json.loads(line))
            except json.JSONDecodeError:
                continue

        if not entries:
            return "(no conversation history yet)"

        parts: list[str] = []
        for e in entries:
            tag = f"[{e['timestamp']}] [{e['channel']}:{e['chat_id']}]"
            if e["role"] == "user":
                parts.append(f"{tag} {e['sender']}: {e['text']}")
            else:
                parts.append(f"{tag} You: {e['text']}")
        return "\n".join(parts)
    except Exception:
        logger.warning("failed to load chat context", exc_info=True)
        return "(no conversation history yet)"
