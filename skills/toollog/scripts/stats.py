#!/usr/bin/env python3
"""Get tool call statistics from the nexal database."""

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

    row = conn.execute("SELECT COUNT(*) as total FROM tool_calls").fetchone()
    result["total_tool_calls"] = row["total"]

    rows = conn.execute(
        "SELECT tool_name, COUNT(*) as count, "
        "SUM(CASE WHEN status='error' THEN 1 ELSE 0 END) as errors, "
        "ROUND(AVG(duration_ms)) as avg_duration_ms "
        "FROM tool_calls GROUP BY tool_name ORDER BY count DESC"
    ).fetchall()
    result["tool_call_stats"] = [dict(r) for r in rows]

    rows = conn.execute(
        "SELECT channel, COUNT(*) as count FROM tool_calls GROUP BY channel ORDER BY count DESC"
    ).fetchall()
    result["tool_calls_by_channel"] = [dict(r) for r in rows]

    conn.close()
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
