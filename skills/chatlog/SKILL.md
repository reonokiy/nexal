---
name: chatlog
description: |
  Query conversation history from the nexal database.
  Use to: (1) Look up past conversations, (2) Search message history,
  (3) Get message statistics.
metadata:
  always_load: true
---

# Chatlog Skill

Conversation messages are queried via the DB API proxy. Read-only, structured endpoints only.

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
```

## Examples

```bash
# Who messaged today?
python3 ./scripts/query_messages.py --since "2025-01-15T00:00:00" --role user

# Search for a topic in chat history
python3 ./scripts/query_messages.py --search "deployment" --limit 20

# Message stats overview
python3 ./scripts/stats.py
```
