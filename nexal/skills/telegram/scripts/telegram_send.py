#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "requests>=2.31.0",
#     "telegramify-markdown>=0.5.0",
# ]
# ///

"""Send messages via Telegram Bot API with MarkdownV2 conversion."""

import argparse
import os
import sys

import requests

try:
    from telegramify_markdown import markdownify
except ImportError:
    print("Error: telegramify_markdown not installed. Run: pip install telegramify-markdown")
    sys.exit(1)


def unescape_newlines(text: str) -> str:
    result = text.replace("\\n", "\n")
    result = result.replace("\\r\\n", "\r\n")
    result = result.replace("\\r", "\r")
    return result


def send_message(
    bot_token: str,
    chat_id: str,
    text: str,
    reply_to_message_id: int | None = None,
    mention_username: str | None = None,
) -> dict:
    url = f"https://api.telegram.org/bot{bot_token}/sendMessage"

    text = unescape_newlines(text)
    if mention_username:
        text = f"@{mention_username} {text}"

    converted_text = markdownify(text).rstrip("\n")

    payload = {
        "chat_id": chat_id,
        "text": converted_text,
        "parse_mode": "MarkdownV2",
    }

    if reply_to_message_id:
        payload["reply_to_message_id"] = reply_to_message_id

    response = requests.post(url, json=payload, timeout=30)
    if response.status_code == 400 and reply_to_message_id:
        # Reply target may have been deleted; retry without reply.
        payload.pop("reply_to_message_id", None)
        response = requests.post(url, json=payload, timeout=30)
    if not response.ok:
        response.raise_for_status()

    return response.json()


def main():
    parser = argparse.ArgumentParser(description="Send messages via Telegram Bot API")
    parser.add_argument("--chat-id", "-c", required=True, help="Target chat ID")
    parser.add_argument("--message", "-m", required=True, help="Message text (markdown supported)")
    parser.add_argument("--token", "-t", help="Bot token (defaults to TELEGRAM_BOT_TOKEN env var)")
    parser.add_argument("--reply-to", "-r", type=int, help="Message ID to reply to")
    parser.add_argument("--source-is-bot", action="store_true", help="Source sender is a bot; use @username style")
    parser.add_argument("--source-username", help="Username for @mention when --source-is-bot is set")

    args = parser.parse_args()

    bot_token = args.token or os.environ.get("TELEGRAM_BOT_TOKEN")
    if not bot_token:
        print("Error: Bot token required. Set TELEGRAM_BOT_TOKEN env var or use --token")
        sys.exit(1)

    chat_id = args.chat_id.strip()
    reply_to = args.reply_to
    mention_username = None
    if args.source_is_bot:
        if not args.source_username:
            print("Error: --source-username is required when --source-is-bot is set")
            sys.exit(1)
        reply_to = None
        mention_username = args.source_username

    try:
        send_message(bot_token, chat_id, args.message, reply_to, mention_username)
        print(f"Message sent to {chat_id}")
    except requests.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.text}")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
