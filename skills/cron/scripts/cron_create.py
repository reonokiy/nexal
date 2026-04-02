#!/usr/bin/env python3
"""Create a new cron job in the database."""

import argparse
import sqlite3
import sys
import uuid
import time

DB_PATH = "/workspace/agents/nexal.db"


def main():
    parser = argparse.ArgumentParser(description="Create a cron job")
    parser.add_argument("--schedule", "-s", required=True,
                        help="Cron expression, 'every:SECONDS', or 'once:ISO8601'")
    parser.add_argument("--label", "-l", required=True, help="Human-readable label")
    parser.add_argument("--message", "-m", required=True,
                        help="Message injected when job fires")
    parser.add_argument("--target-channel", required=True,
                        help="Target channel (telegram, http, etc.)")
    parser.add_argument("--target-chat-id", required=True,
                        help="Target chat ID within the channel")
    parser.add_argument("--context", default="",
                        help="Optional context to save with the job")

    args = parser.parse_args()

    job_id = str(uuid.uuid4())[:8]
    now_ms = int(time.time() * 1000)

    try:
        conn = sqlite3.connect(DB_PATH)
        conn.execute(
            """INSERT INTO cron_jobs
               (id, label, schedule, message, target_channel, target_chat_id, context, enabled, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?)""",
            (job_id, args.label, args.schedule, args.message,
             args.target_channel, args.target_chat_id, args.context, now_ms),
        )
        conn.commit()
        conn.close()
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    print(f"Created job {job_id}: {args.label}")
    print(f"  Schedule: {args.schedule}")
    print(f"  Target: {args.target_channel}:{args.target_chat_id}")


if __name__ == "__main__":
    main()
