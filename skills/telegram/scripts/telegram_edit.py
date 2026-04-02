#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "telegramify-markdown>=0.5.0",
# ]
# ///

"""Edit an existing Telegram message through the nexal token proxy."""

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
STATE_SIGNAL_SOCK = "/workspace/agents/.state"


def _signal_idle(chat_id: str):
    """Send BUSY→IDLE state transition signal via the state signal socket."""
    try:
        sig_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sig_sock.connect(STATE_SIGNAL_SOCK)
        payload = json.dumps({"session": f"telegram:{chat_id}", "state": "IDLE"})
        sig_sock.sendall((payload + "\n").encode())
        sig_sock.close()
    except Exception:
        pass


def unescape_newlines(text: str) -> str:
    result = text.replace("\\n", "\n")
    result = result.replace("\\r\\n", "\r\n")
    result = result.replace("\\r", "\r")
    return result


def _proxy_post(method: str, data: dict) -> dict:
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


def edit_message(chat_id: str, message_id: int, text: str) -> dict:
    text = unescape_newlines(text)

    if HAS_MARKDOWNIFY:
        converted_text = markdownify(text).rstrip("\n")
        parse_mode = "MarkdownV2"
    else:
        converted_text = text
        parse_mode = ""

    payload: dict = {
        "chat_id": chat_id,
        "message_id": message_id,
        "text": converted_text,
    }
    if parse_mode:
        payload["parse_mode"] = parse_mode

    return _proxy_post("editMessageText", payload)


def main():
    parser = argparse.ArgumentParser(description="Edit a Telegram message")
    parser.add_argument("--chat-id", "-c", required=True, help="Target chat ID")
    parser.add_argument("--message-id", "-m", type=int, required=True, help="Message ID to edit")
    parser.add_argument("--text", "-t", required=True, help="New message text (markdown supported)")

    args = parser.parse_args()

    try:
        edit_message(args.chat_id, args.message_id, args.text)
        _signal_idle(args.chat_id)
        print(f"Message {args.message_id} edited")
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
