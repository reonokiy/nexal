#!/usr/bin/env python3
"""Delete a cron job by ID from the database."""

import argparse
import sqlite3
import sys

DB_PATH = "/workspace/agents/nexal.db"


def main():
    parser = argparse.ArgumentParser(description="Delete a cron job")
    parser.add_argument("--job-id", "-j", required=True, help="Job ID to delete")
    args = parser.parse_args()

    try:
        conn = sqlite3.connect(DB_PATH)
        cursor = conn.execute("DELETE FROM cron_jobs WHERE id = ?", (args.job_id,))
        conn.commit()
        deleted = cursor.rowcount > 0
        conn.close()
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    if deleted:
        print(f"Deleted job {args.job_id}")
    else:
        print(f"Job {args.job_id} not found.")
        sys.exit(1)


if __name__ == "__main__":
    main()
