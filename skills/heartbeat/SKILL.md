---
name: heartbeat
description: |
  Heartbeat channel — periodic check-in for proactive agent behavior.
metadata:
  channel: heartbeat
---

# Heartbeat Skill

You receive periodic heartbeat messages. Use these to proactively check on tasks, send reminders, or surface anything important across all channels.

## What to Do on Heartbeat

1. Check if you have any pending tasks or follow-ups
2. Review recent conversations for anything that needs attention
3. If there's something to do, use the appropriate channel's send script (e.g. telegram_send.py)
4. If there's nothing to do, call `no_response.sh`

## No Response (Explicit Skip)

```bash
./scripts/no_response.sh
```

You **MUST** either take an action or call `no_response.sh`. Do not silently ignore heartbeats.
