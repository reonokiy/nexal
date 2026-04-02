---
name: cron
description: |
  Schedule future wake-ups and recurring tasks via cron jobs.
metadata:
  always_load: true
---

# Cron Skill

You can schedule your own future wake-ups. Jobs are persisted and survive restarts.

## Schedule Types

- **Cron expression**: standard 5-field crontab (e.g. `0 */2 * * *` = every 2 hours)
- **Interval**: `every:SECONDS` (e.g. `every:300` = every 5 minutes)
- **One-shot**: `once:ISO8601` (e.g. `once:2026-04-02T18:00:00Z` = fire once at that time)

## Command Templates

```bash
# Create a recurring job (cron expression)
uv run ./scripts/cron_create.py \
  --schedule "0 */2 * * *" \
  --label "check-prs" \
  --message "Check for new PR reviews and report status" \
  --target-channel telegram \
  --target-chat-id <CHAT_ID>

# Create an interval job (every 5 minutes)
uv run ./scripts/cron_create.py \
  --schedule "every:300" \
  --label "monitor-deploy" \
  --message "Check deployment status" \
  --target-channel telegram \
  --target-chat-id <CHAT_ID>

# Create a one-shot reminder
uv run ./scripts/cron_create.py \
  --schedule "once:2026-04-02T18:00:00Z" \
  --label "meeting-reminder" \
  --message "Remind about the standup meeting" \
  --target-channel telegram \
  --target-chat-id <CHAT_ID> \
  --context "User asked to be reminded about 6pm standup"

# List all jobs
uv run ./scripts/cron_list.py

# Delete a job
uv run ./scripts/cron_delete.py --job-id <JOB_ID>
```

## Important Notes

- Jobs fire into the **target session** (e.g. `telegram:-12345`), so you have conversation context
- One-shot jobs are automatically deleted after firing
- The `--context` flag saves recent context with the job so you remember why you set it
- Always include a clear `--message` that tells your future self what to do
