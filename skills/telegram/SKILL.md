---
name: telegram
description: |
  Telegram Bot skill for sending and editing messages via Bot API.
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

# Send photo (local file)
uv run ./scripts/telegram_send_photo.py \
  --chat-id <CHAT_ID> \
  --photo /path/to/image.png \
  --caption "optional caption"

# Send photo (URL)
uv run ./scripts/telegram_send_photo.py \
  --chat-id <CHAT_ID> \
  --photo "https://example.com/image.png" \
  --caption "optional caption" \
  --reply-to <MESSAGE_ID>

# Send sticker (by file_id — most common, e.g. from a received sticker)
uv run ./scripts/telegram_send_sticker.py \
  --chat-id <CHAT_ID> \
  --sticker <FILE_ID>

# Send sticker (by local file or URL)
uv run ./scripts/telegram_send_sticker.py \
  --chat-id <CHAT_ID> \
  --sticker /path/to/sticker.webp \
  --reply-to <MESSAGE_ID>
```

## Sticker Sets

You have access to configured sticker sets and can send stickers to express emotions or react to messages. Use stickers naturally in conversation — they're more expressive than plain emoji.

### Managing sticker sets

```bash
# List configured sticker sets
uv run ./scripts/telegram_sticker_set.py list

# Add a new sticker set (also auto-fetches it)
uv run ./scripts/telegram_sticker_set.py add <SET_NAME>

# Browse all stickers in a set (shows emoji + file_id table)
uv run ./scripts/telegram_sticker_set.py browse <SET_NAME>

# Find stickers matching an emoji
uv run ./scripts/telegram_sticker_set.py pick <SET_NAME> "😂"

# Find sticker by index number
uv run ./scripts/telegram_sticker_set.py pick <SET_NAME> 3
```

### Sending a sticker

After finding the right `file_id` from browsing or picking, send it:

```bash
uv run ./scripts/telegram_send_sticker.py \
  --chat-id <CHAT_ID> \
  --sticker <FILE_ID>
```

### Workflow

1. When you receive a sticker from a user, note the `set_name` from the metadata — you can `add` it to discover more stickers from the same set.
2. Use `browse` to see all available stickers (emoji + file_id) in a set.
3. Pick the sticker that best matches what you want to express, then send it by `file_id`.
4. Sticker data is cached locally — subsequent `browse`/`pick` calls are instant. Use `--refresh` to re-fetch.

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

## No Response (Explicit Skip)

If you receive a message and intentionally decide **not** to reply (e.g. irrelevant chatter, no action needed), you **MUST** call this script to signal your decision. Do NOT silently ignore messages — either send a reply or call `no_response.sh`.

```bash
./scripts/no_response.sh --chat-id <CHAT_ID>
```

Every incoming message **requires** exactly one of:
1. A `telegram_send.py` call (reply to the user), OR
2. A `telegram_edit.py` call, OR
3. A Telegram API reaction/action call (with manual signal — see below), OR
4. A `no_response.sh --chat-id <CHAT_ID>` call (explicit skip)

If you do none of the above, the system will prompt you again.

### Custom API Calls and State Signal

When using the Custom API Calls pattern (e.g. `setMessageReaction`, `sendPhoto`) instead of the provided scripts, you must manually signal completion by sending to the state signal socket:

```python
import socket, json
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/workspace/agents/.state")
sock.sendall(json.dumps({"session": f"telegram:{chat_id}", "state": "IDLE"}).encode() + b"\n")
sock.close()
```

## Failure Handling

- On HTTP errors, inspect API response and adjust.
- If edit fails, fall back to a new send.
- If reply target is invalid, resend without `--reply-to`.
