---
name: discord
description: |
  Discord Bot skill for sending messages to channels.
  Use to: (1) Send messages to Discord channels, (2) Send embed messages,
  (3) Any Discord outbound communication.
metadata:
  channel: discord
---

# Discord Skill

Messages are sent through the nexal proxy (via Unix socket). No bot token is needed in the sandbox.

## Command Templates

Paths are relative to this skill directory.

```bash
# Send message
uv run ./scripts/discord_send.py \
  --channel <CHANNEL_ID> \
  --message "<TEXT>"
```

## Response Contract

- Return only the final message content.
- Do not include action narration or meta text.
- Keep messages concise unless detail is requested.
