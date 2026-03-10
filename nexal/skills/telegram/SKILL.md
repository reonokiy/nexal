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

Assumption: `TELEGRAM_BOT_TOKEN` is already available in environment.

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

For other Telegram Bot API actions, use `curl` directly.

## Failure Handling

- On HTTP errors, inspect API response and adjust.
- If edit fails, fall back to a new send.
- If reply target is invalid, resend without `--reply-to`.
