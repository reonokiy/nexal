#!/usr/bin/env python3
"""Query tool call records from the nexal database."""

import argparse
import json
import sqlite3
import sys

DB_PATH = "/workspace/agents/nexal.db"


def query(args: argparse.Namespace) -> None:
    try:
        conn = sqlite3.connect(f"file:{DB_PATH}?mode=ro", uri=True)
    except sqlite3.OperationalError:
        print(json.dumps({"error": f"Cannot open database: {DB_PATH}"}))
        sys.exit(1)

    conn.row_factory = sqlite3.Row

    clauses: list[str] = []
    params: list[object] = []

    if args.channel:
        clauses.append("channel = ?")
        params.append(args.channel)
    if args.chat_id:
        clauses.append("chat_id = ?")
        params.append(args.chat_id)
    if args.tool_name:
        clauses.append("tool_name = ?")
        params.append(args.tool_name)
    if args.status:
        clauses.append("status = ?")
        params.append(args.status)
    if args.since:
        clauses.append("timestamp >= ?")
        params.append(args.since)
    if args.until:
        clauses.append("timestamp <= ?")
        params.append(args.until)

    where = (" WHERE " + " AND ".join(clauses)) if clauses else ""
    sql = f"SELECT * FROM tool_calls{where} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
    params.extend([args.limit, args.offset])

    rows = conn.execute(sql, params).fetchall()
    conn.close()

    results = [dict(r) for r in reversed(rows)]
    print(json.dumps(results, ensure_ascii=False, indent=2))


def main() -> None:
    parser = argparse.ArgumentParser(description="Query tool call records")
    parser.add_argument("--channel", help="Filter by channel (e.g. telegram, discord, cli, agent)")
    parser.add_argument("--chat-id", help="Filter by chat/conversation ID")
    parser.add_argument("--tool-name", help="Filter by tool name (e.g. exec, web_search)")
    parser.add_argument("--status", choices=["ok", "error"], help="Filter by status")
    parser.add_argument("--since", help="ISO timestamp lower bound (inclusive)")
    parser.add_argument("--until", help="ISO timestamp upper bound (inclusive)")
    parser.add_argument("--limit", type=int, default=50, help="Max rows (default: 50)")
    parser.add_argument("--offset", type=int, default=0, help="Skip first N results")
    args = parser.parse_args()
    query(args)


if __name__ == "__main__":
    main()
