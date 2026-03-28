---
name: chatlog
description: |
  Query conversation history from the nexal database.
  Use to: (1) Look up past conversations, (2) Search message history,
  (3) Get message statistics, (4) Run custom read-only SQL.
metadata:
  always_load: true
---

# Chatlog Skill

The nexal database at `/workspace/agents/nexal.db` stores conversation messages. It is read-only from the sandbox.

## Schema

**messages** table:
- `id`, `channel`, `chat_id`, `sender`, `role` (user/assistant), `text`, `timestamp`, `metadata` (JSON)

## Command Templates

```bash
# Query messages — all filters are optional
python3 ./scripts/query_messages.py \
  --channel <CHANNEL> \
  --chat-id <CHAT_ID> \
  --sender <SENDER_NAME> \
  --role user|assistant \
  --since <ISO_TIMESTAMP> \
  --until <ISO_TIMESTAMP> \
  --search "<TEXT>" \
  --limit 50 --offset 0

# Get message statistics
python3 ./scripts/stats.py

# Run custom read-only SQL (works on all tables)
python3 ./scripts/raw_sql.py "SELECT sender, COUNT(*) as n FROM messages GROUP BY sender ORDER BY n DESC"
```

## Examples

```bash
# Who messaged today?
python3 ./scripts/query_messages.py --since "2025-01-15T00:00:00" --role user

# How many messages per channel?
python3 ./scripts/raw_sql.py "SELECT channel, COUNT(*) as n FROM messages GROUP BY channel"

# Search for a topic in chat history
python3 ./scripts/query_messages.py --search "deployment" --limit 20
```
