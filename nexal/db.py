"""SQLite-backed chat history with per-channel storage and query support."""

from __future__ import annotations

import json
import logging
import sqlite3
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from nexal.settings import settings

logger = logging.getLogger("nexal.db")

_AGENTS_DIR = "agents"

_SCHEMA = """\
CREATE TABLE IF NOT EXISTS messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    channel     TEXT    NOT NULL,
    chat_id     TEXT    NOT NULL,
    sender      TEXT    NOT NULL,
    role        TEXT    NOT NULL,
    text        TEXT    NOT NULL,
    timestamp   TEXT    NOT NULL,
    metadata    TEXT    NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_messages_channel_chat
    ON messages (channel, chat_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_messages_timestamp
    ON messages (timestamp);
CREATE INDEX IF NOT EXISTS idx_messages_sender
    ON messages (sender);

CREATE TABLE IF NOT EXISTS tool_calls (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    channel         TEXT    NOT NULL,
    chat_id         TEXT    NOT NULL,
    tool_call_id    TEXT    NOT NULL,
    tool_name       TEXT    NOT NULL,
    arguments       TEXT    NOT NULL DEFAULT '{}',
    output          TEXT    NOT NULL DEFAULT '',
    status          TEXT    NOT NULL DEFAULT 'ok',
    duration_ms     INTEGER,
    timestamp       TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tool_calls_channel_chat
    ON tool_calls (channel, chat_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_tool_calls_tool_name
    ON tool_calls (tool_name);
CREATE INDEX IF NOT EXISTS idx_tool_calls_timestamp
    ON tool_calls (timestamp);
"""

_local = threading.local()


def _db_path() -> Path:
    workspace = settings.sandbox_workspace_dir
    if not workspace:
        raise RuntimeError("sandbox_workspace_dir is not set")
    return Path(workspace) / _AGENTS_DIR / "nexal.db"


def _get_conn() -> sqlite3.Connection:
    """Return a per-thread SQLite connection (created on first access)."""
    conn: sqlite3.Connection | None = getattr(_local, "conn", None)
    path = _db_path()
    # Reconnect if path changed (e.g. different session).
    if conn is not None and getattr(_local, "path", None) != str(path):
        conn.close()
        conn = None
    if conn is None:
        path.parent.mkdir(parents=True, exist_ok=True)
        conn = sqlite3.connect(str(path), timeout=10)
        conn.row_factory = sqlite3.Row
        conn.executescript(_SCHEMA)
        _local.conn = conn
        _local.path = str(path)
    return conn


# ── Write ────────────────────────────────────────────────────────


def save_chat_entry(
    channel: str,
    chat_id: str,
    sender: str,
    text: str,
    role: str,
    metadata: dict[str, Any] | None = None,
) -> int:
    """Insert a chat message. Returns the row id."""
    now = datetime.now(timezone.utc).isoformat()
    conn = _get_conn()
    cur = conn.execute(
        "INSERT INTO messages (channel, chat_id, sender, role, text, timestamp, metadata)"
        " VALUES (?, ?, ?, ?, ?, ?, ?)",
        (channel, chat_id, sender, role, text, now, json.dumps(metadata or {}, ensure_ascii=False)),
    )
    conn.commit()
    return cur.lastrowid  # type: ignore[return-value]


def save_tool_call(
    channel: str,
    chat_id: str,
    tool_call_id: str,
    tool_name: str,
    arguments: str,
    output: str,
    status: str = "ok",
    duration_ms: int | None = None,
) -> int:
    """Insert a tool call record. Returns the row id."""
    now = datetime.now(timezone.utc).isoformat()
    conn = _get_conn()
    cur = conn.execute(
        "INSERT INTO tool_calls (channel, chat_id, tool_call_id, tool_name, arguments, output, status, duration_ms, timestamp)"
        " VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (channel, chat_id, tool_call_id, tool_name, arguments, output, status, duration_ms, now),
    )
    conn.commit()
    return cur.lastrowid  # type: ignore[return-value]


# ── Read ─────────────────────────────────────────────────────────


def query_tool_calls(
    *,
    channel: str | None = None,
    chat_id: str | None = None,
    tool_name: str | None = None,
    status: str | None = None,
    since: str | None = None,
    until: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> list[dict[str, Any]]:
    """Query tool call records with optional filters."""
    clauses: list[str] = []
    params: list[Any] = []

    if channel:
        clauses.append("channel = ?")
        params.append(channel)
    if chat_id:
        clauses.append("chat_id = ?")
        params.append(chat_id)
    if tool_name:
        clauses.append("tool_name = ?")
        params.append(tool_name)
    if status:
        clauses.append("status = ?")
        params.append(status)
    if since:
        clauses.append("timestamp >= ?")
        params.append(since)
    if until:
        clauses.append("timestamp <= ?")
        params.append(until)

    where = (" WHERE " + " AND ".join(clauses)) if clauses else ""
    sql = f"SELECT * FROM tool_calls{where} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
    params.extend([limit, offset])

    conn = _get_conn()
    rows = conn.execute(sql, params).fetchall()
    return [dict(r) for r in reversed(rows)]


def load_chat_context(
    limit: int = 50,
    channel: str | None = None,
    chat_id: str | None = None,
) -> str:
    """Load recent chat entries as a formatted string for the agent's context window.

    Optionally filter by channel and/or chat_id.
    """
    rows = query_messages(limit=limit, channel=channel, chat_id=chat_id)
    if not rows:
        return "(no conversation history yet)"
    parts: list[str] = []
    for r in rows:
        tag = f"[{r['timestamp']}] [{r['channel']}:{r['chat_id']}]"
        if r["role"] == "user":
            parts.append(f"{tag} {r['sender']}: {r['text']}")
        else:
            parts.append(f"{tag} You: {r['text']}")
    return "\n".join(parts)


def query_messages(
    *,
    channel: str | None = None,
    chat_id: str | None = None,
    sender: str | None = None,
    role: str | None = None,
    since: str | None = None,
    until: str | None = None,
    search: str | None = None,
    limit: int = 50,
    offset: int = 0,
) -> list[dict[str, Any]]:
    """Flexible message query.

    Parameters
    ----------
    channel : filter by channel name (e.g. "telegram", "cli")
    chat_id : filter by chat/conversation id
    sender  : filter by sender name
    role    : filter by role ("user" or "assistant")
    since   : ISO timestamp lower bound (inclusive)
    until   : ISO timestamp upper bound (inclusive)
    search  : substring search in message text (case-insensitive)
    limit   : max rows to return (default 50)
    offset  : skip first N results
    """
    clauses: list[str] = []
    params: list[Any] = []

    if channel:
        clauses.append("channel = ?")
        params.append(channel)
    if chat_id:
        clauses.append("chat_id = ?")
        params.append(chat_id)
    if sender:
        clauses.append("sender = ?")
        params.append(sender)
    if role:
        clauses.append("role = ?")
        params.append(role)
    if since:
        clauses.append("timestamp >= ?")
        params.append(since)
    if until:
        clauses.append("timestamp <= ?")
        params.append(until)
    if search:
        clauses.append("text LIKE ?")
        params.append(f"%{search}%")

    where = (" WHERE " + " AND ".join(clauses)) if clauses else ""
    sql = f"SELECT * FROM messages{where} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
    params.extend([limit, offset])

    conn = _get_conn()
    rows = conn.execute(sql, params).fetchall()
    # Return in chronological order.
    return [dict(r) for r in reversed(rows)]


def count_messages(
    *,
    channel: str | None = None,
    chat_id: str | None = None,
) -> int:
    """Count messages, optionally filtered by channel/chat_id."""
    clauses: list[str] = []
    params: list[Any] = []
    if channel:
        clauses.append("channel = ?")
        params.append(channel)
    if chat_id:
        clauses.append("chat_id = ?")
        params.append(chat_id)
    where = (" WHERE " + " AND ".join(clauses)) if clauses else ""
    conn = _get_conn()
    return conn.execute(f"SELECT COUNT(*) FROM messages{where}", params).fetchone()[0]
