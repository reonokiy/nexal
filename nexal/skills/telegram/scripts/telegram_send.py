#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "telegramify-markdown>=0.5.0",
# ]
# ///

"""Send messages via Telegram through the nexal token proxy."""

import argparse
import http.client
import json
import socket
import sys

try:
    from telegramify_markdown import markdownify
    HAS_MARKDOWNIFY = True
except ImportError:
    HAS_MARKDOWNIFY = False

PROXY_SOCK = "/workspace/agents/proxy/api.telegram.org"


def unescape_newlines(text: str) -> str:
    result = text.replace("\\n", "\n")
    result = result.replace("\\r\\n", "\r\n")
    result = result.replace("\\r", "\r")
    return result


def _proxy_post(method: str, data: dict) -> dict:
    """POST to the Telegram proxy. method is the Bot API method name (e.g. sendMessage)."""
    body = json.dumps(data, ensure_ascii=False).encode()
    conn = http.client.HTTPConnection("localhost")
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(PROXY_SOCK)
    conn.sock = sock
    try:
        conn.request("POST", f"/{method}", body=body,
                      headers={"Content-Type": "application/json"})
        resp = conn.getresponse()
        result = json.loads(resp.read())
        if resp.status >= 400:
            print(f"API error ({resp.status}): {json.dumps(result)}", file=sys.stderr)
            sys.exit(1)
        return result
    finally:
        conn.close()


def send_message(
    chat_id: str,
    text: str,
    reply_to_message_id: int | None = None,
    mention_username: str | None = None,
) -> dict:
    text = unescape_newlines(text)
    if mention_username:
        text = f"@{mention_username} {text}"

    if HAS_MARKDOWNIFY:
        converted_text = markdownify(text).rstrip("\n")
        parse_mode = "MarkdownV2"
    else:
        converted_text = text
        parse_mode = ""

    payload: dict = {
        "chat_id": chat_id,
        "text": converted_text,
    }
    if parse_mode:
        payload["parse_mode"] = parse_mode
    if reply_to_message_id:
        payload["reply_to_message_id"] = reply_to_message_id

    result = _proxy_post("sendMessage", payload)

    # If reply target was invalid, retry without it.
    if not result.get("ok") and reply_to_message_id:
        payload.pop("reply_to_message_id", None)
        result = _proxy_post("sendMessage", payload)

    return result


def main():
    parser = argparse.ArgumentParser(description="Send messages via Telegram")
    parser.add_argument("--chat-id", "-c", required=True, help="Target chat ID")
    parser.add_argument("--message", "-m", required=True, help="Message text (markdown supported)")
    parser.add_argument("--reply-to", "-r", type=int, help="Message ID to reply to")
    parser.add_argument("--source-is-bot", action="store_true", help="Source sender is a bot; use @username style")
    parser.add_argument("--source-username", help="Username for @mention when --source-is-bot is set")

    args = parser.parse_args()

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
        send_message(chat_id, args.message, reply_to, mention_username)
        print(f"Message sent to {chat_id}")
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
