---
name: http
description: |
  HTTP channel skill for sending responses via the response socket.
metadata:
  channel: http
---

# HTTP Skill

Responses are sent through a Unix socket at `/workspace/agents/proxy/http.channel`. No auth needed.

## Required Inputs

- `chat_id` (required)
- message text (required)

## Command Templates

Paths are relative to this skill directory.

```bash
# Send message
uv run ./scripts/http_send.py \
  --chat-id <CHAT_ID> \
  --message "<TEXT>"
```

## No Response (Explicit Skip)

If you intentionally decide **not** to reply, you **MUST** call this script:

```bash
./scripts/no_response.sh --chat-id <CHAT_ID>
```

Every incoming message **requires** exactly one of:
1. An `http_send.py` call (reply to the user), OR
2. A `no_response.sh --chat-id <CHAT_ID>` call (explicit skip)

If you do none of the above, the system will prompt you again.
