---
name: toollog
description: |
  Query tool call records from the nexal database.
  Use to: (1) Review tool execution history, (2) Find failed tool calls,
  (3) Analyze tool usage and performance.
metadata:
  always_load: true
---

# Toollog Skill

Tool call records are queried via the DB API proxy. Read-only, structured endpoints only.

## Command Templates

```bash
# Query tool calls — all filters are optional
python3 ./scripts/query.py \
  --channel <CHANNEL> \
  --chat-id <CHAT_ID> \
  --tool-name <TOOL_NAME> \
  --status ok|error \
  --since <ISO_TIMESTAMP> \
  --until <ISO_TIMESTAMP> \
  --limit 50 --offset 0

# Get tool call statistics
python3 ./scripts/stats.py
```

## Examples

```bash
# What tools failed recently?
python3 ./scripts/query.py --status error --limit 10

# Recent web_search calls
python3 ./scripts/query.py --tool-name web_search --limit 10

# Performance overview
python3 ./scripts/stats.py
```
