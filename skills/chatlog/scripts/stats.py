#!/usr/bin/env python3
"""Get message statistics from the nexal database."""

import json
import sqlite3
import sys

DB_PATH = "/workspace/agents/nexal.db"


def main() -> None:
    try:
        conn = sqlite3.connect(f"file:{DB_PATH}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        print(json.dumps({"error": f"Cannot open database: {DB_PATH}"}))
        sys.exit(1)

    conn.row_factory = sqlite3.Row
    result: dict = {}

    row = conn.execute("SELECT COUNT(*) as total FROM messages").fetchone()
    result["total_messages"] = row["total"]

    rows = conn.execute(
        "SELECT channel, role, COUNT(*) as count FROM messages GROUP BY channel, role ORDER BY channel, role"
    ).fetchall()
    result["messages_by_channel_role"] = [dict(r) for r in rows]

    rows = conn.execute(
        "SELECT sender, COUNT(*) as count FROM messages GROUP BY sender ORDER BY count DESC LIMIT 20"
    ).fetchall()
    result["top_senders"] = [dict(r) for r in rows]

    conn.close()
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
