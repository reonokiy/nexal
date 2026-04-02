#!/usr/bin/env python3
"""List all cron jobs from the database."""

import sqlite3
import sys

DB_PATH = "/workspace/agents/nexal.db"


def main():
    try:
        conn = sqlite3.connect(DB_PATH)
        rows = conn.execute(
            "SELECT id, label, schedule, target_channel, target_chat_id, enabled, last_run_at FROM cron_jobs ORDER BY created_at"
        ).fetchall()
        conn.close()
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    if not rows:
        print("No cron jobs configured.")
        return

    print(f"{'ID':<10} {'Label':<20} {'Schedule':<25} {'Target':<25} {'Enabled':<8} {'Last Run'}")
    print("-" * 110)

    for row in rows:
        job_id, label, schedule, target_ch, target_chat, enabled, last_run_ms = row
        target = f"{target_ch}:{target_chat}"
        enabled_str = "yes" if enabled else "no"
        if last_run_ms:
            from datetime import datetime, timezone
            last_run = datetime.fromtimestamp(last_run_ms / 1000, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
        else:
            last_run = "never"
        print(f"{job_id:<10} {label:<20} {schedule:<25} {target:<25} {enabled_str:<8} {last_run}")


if __name__ == "__main__":
    main()
