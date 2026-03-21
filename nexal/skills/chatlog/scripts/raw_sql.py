#!/usr/bin/env python3
"""Execute a read-only SQL query against the nexal database."""

import argparse
import json
import sqlite3
import sys

DB_PATH = "/workspace/agents/nexal.db"


_MAX_ROWS = 500


def run_query(sql: str, limit: int = _MAX_ROWS) -> None:
    # Reject statements that aren't SELECT/WITH (defense-in-depth; mode=ro blocks writes too).
    stripped = sql.strip().lower()
    if not stripped.startswith(("select", "with", "pragma", "explain")):
        print(json.dumps({"error": "Only SELECT / WITH / PRAGMA / EXPLAIN queries are allowed"}))
        sys.exit(1)

    try:
        conn = sqlite3.connect(f"file:{DB_PATH}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        print(json.dumps({"error": f"Cannot open database: {DB_PATH}"}))
        sys.exit(1)

    conn.row_factory = sqlite3.Row

    try:
        cursor = conn.execute(sql)
        rows = cursor.fetchmany(limit)
        results = [dict(r) for r in rows]
        if len(results) == limit:
            results.append({"_truncated": True, "_message": f"Results limited to {limit} rows"})
        print(json.dumps(results, ensure_ascii=False, indent=2))
    except sqlite3.Error as e:
        print(json.dumps({"error": str(e)}))
        sys.exit(1)
    finally:
        conn.close()


def main() -> None:
    parser = argparse.ArgumentParser(description="Run read-only SQL on nexal database")
    parser.add_argument("sql", help="SQL query to execute (SELECT only)")
    args = parser.parse_args()
    run_query(args.sql)


if __name__ == "__main__":
    main()
