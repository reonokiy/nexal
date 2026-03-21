#!/usr/bin/env python3
"""Query chat messages from the chatlog database."""

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
    if args.sender:
        clauses.append("sender = ?")
        params.append(args.sender)
    if args.role:
        clauses.append("role = ?")
        params.append(args.role)
    if args.since:
        clauses.append("timestamp >= ?")
        params.append(args.since)
    if args.until:
        clauses.append("timestamp <= ?")
        params.append(args.until)
    if args.search:
        clauses.append("text LIKE ?")
        params.append(f"%{args.search}%")

    where = (" WHERE " + " AND ".join(clauses)) if clauses else ""
    sql = f"SELECT * FROM messages{where} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
    params.extend([args.limit, args.offset])

    rows = conn.execute(sql, params).fetchall()
    conn.close()

    results = [dict(r) for r in reversed(rows)]
    print(json.dumps(results, ensure_ascii=False, indent=2))


def main() -> None:
    parser = argparse.ArgumentParser(description="Query chat messages")
    parser.add_argument("--channel", help="Filter by channel (e.g. telegram, discord, cli)")
    parser.add_argument("--chat-id", help="Filter by chat/conversation ID")
    parser.add_argument("--sender", help="Filter by sender name")
    parser.add_argument("--role", choices=["user", "assistant"], help="Filter by role")
    parser.add_argument("--since", help="ISO timestamp lower bound (inclusive)")
    parser.add_argument("--until", help="ISO timestamp upper bound (inclusive)")
    parser.add_argument("--search", help="Substring search in message text (case-insensitive)")
    parser.add_argument("--limit", type=int, default=50, help="Max rows (default: 50)")
    parser.add_argument("--offset", type=int, default=0, help="Skip first N results")
    args = parser.parse_args()
    query(args)


if __name__ == "__main__":
    main()
