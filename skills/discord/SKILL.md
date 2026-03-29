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

## Custom API Calls

For any Discord API endpoint not covered by the script above, call the proxy socket directly. Auth is injected automatically.

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///
import httpx, json, sys

PROXY = "/workspace/agents/proxy/discord.com"
transport = httpx.HTTPTransport(uds=PROXY)
client = httpx.Client(transport=transport, base_url="http://localhost")

# Example: send embed
resp = client.post(f"/api/v10/channels/{sys.argv[1]}/messages", json={
    "embeds": [{"title": "Hello", "description": sys.argv[2]}]
})
print(json.dumps(resp.json(), indent=2))
```

Run with `uv run script.py <channel_id> <description>`. Works for any Discord API endpoint.

## Response Contract

- Return only the final message content.
- Do not include action narration or meta text.
- Keep messages concise unless detail is requested.
