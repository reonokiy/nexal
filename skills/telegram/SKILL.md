---
name: telegram
description: |
  Telegram Bot skill for sending and editing messages via Bot API.
  Use to: (1) Send messages to Telegram users/groups, (2) Reply to specific messages,
  (3) Edit existing messages, (4) Push proactive notifications.
metadata:
  channel: telegram
---

# Telegram Skill

Messages are sent through the nexal proxy (via Unix socket at `/workspace/agents/proxy/api.telegram.org`). No bot token is needed in the sandbox.

## Required Inputs

- `chat_id` (required)
- message content (required)
- `reply_to_message_id` (optional, for threaded reply)
- `message_id` (required for edit)

## Execution Policy

1. If handling a direct user message and `message_id` is known, prefer reply mode (`--reply-to`).
2. If source sender is a bot (`sender_is_bot=true`), do not use reply mode — use `@<username>` prefix instead.
3. For long-running tasks, send a progress message then edit it with final status.
4. Use markdown for formatting, not HTML.

## Command Templates

Paths are relative to this skill directory.

```bash
# Send message
uv run ./scripts/telegram_send.py \
  --chat-id <CHAT_ID> \
  --message "<TEXT>"

# Reply to a specific message
uv run ./scripts/telegram_send.py \
  --chat-id <CHAT_ID> \
  --message "<TEXT>" \
  --reply-to <MESSAGE_ID>

# Source is bot: use @username style
uv run ./scripts/telegram_send.py \
  --chat-id <CHAT_ID> \
  --message "<TEXT>" \
  --source-is-bot \
  --source-username <USERNAME>

# Edit existing message
uv run ./scripts/telegram_edit.py \
  --chat-id <CHAT_ID> \
  --message-id <MESSAGE_ID> \
  --text "<TEXT>"
```

## Custom API Calls

For any Telegram Bot API method not covered by the scripts above, call the proxy socket directly. The auth token is injected automatically — just use the API path.

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///
import httpx, json, sys

PROXY = "/workspace/agents/proxy/api.telegram.org"
transport = httpx.HTTPTransport(uds=PROXY)
client = httpx.Client(transport=transport, base_url="http://localhost")

# Example: any Bot API method
resp = client.post("/sendPhoto", json={
    "chat_id": sys.argv[1],
    "photo": sys.argv[2],
    "caption": sys.argv[3] if len(sys.argv) > 3 else ""
})
print(json.dumps(resp.json(), indent=2))
```

Run with `uv run script.py <chat_id> <photo_url> [caption]`. Works for any Bot API method — sendDocument, sendLocation, getUpdates, etc.

## Downloading Files

When you receive a message with a `file_id` (documents, voice, video, audio, stickers), download the file using the Telegram Bot API `getFile` method through the proxy:

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///
import httpx, json, sys

PROXY = "/workspace/agents/proxy/api.telegram.org"
transport = httpx.HTTPTransport(uds=PROXY)
client = httpx.Client(transport=transport, base_url="http://localhost")

file_id = sys.argv[1]
output_path = sys.argv[2] if len(sys.argv) > 2 else "/workspace/downloaded_file"

# Step 1: get file path
resp = client.post("/getFile", json={"file_id": file_id})
file_path = resp.json()["result"]["file_path"]

# Step 2: download (use the Telegram file API directly)
# Note: file downloads go through api.telegram.org/file/bot<token>/<path>
# The proxy handles the /file/ prefix too
file_resp = client.get(f"/file/{file_path}")
with open(output_path, "wb") as f:
    f.write(file_resp.content)

print(f"Downloaded to {output_path} ({len(file_resp.content)} bytes)")
```

Run: `uv run download.py <file_id> [output_path]`

## Failure Handling

- On HTTP errors, inspect API response and adjust.
- If edit fails, fall back to a new send.
- If reply target is invalid, resend without `--reply-to`.
