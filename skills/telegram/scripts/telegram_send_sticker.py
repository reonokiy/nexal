#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Send a sticker via Telegram through the nexal token proxy.

Supports file_id (most common), URL, or local file upload (.webp/.tgs/.webm).
"""

import argparse
import io
import json
import os
import socket
import sys
import uuid

PROXY_SOCK = "/workspace/agents/proxy/api.telegram.org"
STATE_SIGNAL_SOCK = "/workspace/agents/.state"


def _signal_idle(chat_id: str):
    try:
        sig_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sig_sock.connect(STATE_SIGNAL_SOCK)
        payload = json.dumps({"session": f"telegram:{chat_id}", "state": "IDLE"})
        sig_sock.sendall((payload + "\n").encode())
        sig_sock.close()
    except Exception:
        pass


def _send_via_socket(method: str, body: bytes, content_type: str) -> dict:
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(PROXY_SOCK)

    request = (
        f"POST /{method} HTTP/1.1\r\n"
        f"Host: localhost\r\n"
        f"Content-Type: {content_type}\r\n"
        f"Content-Length: {len(body)}\r\n"
        f"Connection: close\r\n"
        f"\r\n"
    ).encode() + body

    sock.sendall(request)

    chunks = []
    while True:
        chunk = sock.recv(8192)
        if not chunk:
            break
        chunks.append(chunk)
    sock.close()

    raw = b"".join(chunks)
    parts = raw.split(b"\r\n\r\n", 1)
    if len(parts) == 2:
        resp_body = parts[1]
    else:
        resp_body = raw

    header_str = parts[0].decode("utf-8", errors="replace") if len(parts) == 2 else ""
    if "transfer-encoding: chunked" in header_str.lower():
        resp_body = _decode_chunked(resp_body)

    return json.loads(resp_body)


def _decode_chunked(data: bytes) -> bytes:
    result = bytearray()
    buf = io.BytesIO(data)
    while True:
        line = buf.readline()
        if not line:
            break
        size_str = line.strip()
        if not size_str:
            continue
        try:
            chunk_size = int(size_str, 16)
        except ValueError:
            break
        if chunk_size == 0:
            break
        result.extend(buf.read(chunk_size))
        buf.readline()
    return bytes(result)


def send_sticker_id(chat_id: str, sticker: str, reply_to: int | None = None, emoji: str | None = None) -> dict:
    """Send a sticker by file_id or URL (JSON API)."""
    payload: dict = {
        "chat_id": chat_id,
        "sticker": sticker,
    }
    if reply_to:
        payload["reply_to_message_id"] = reply_to
    if emoji:
        payload["emoji"] = emoji

    body = json.dumps(payload, ensure_ascii=False).encode()
    return _send_via_socket("sendSticker", body, "application/json")


def send_sticker_file(chat_id: str, file_path: str, reply_to: int | None = None, emoji: str | None = None) -> dict:
    """Send a sticker by uploading a local file (multipart/form-data)."""
    boundary = f"----FormBoundary{uuid.uuid4().hex[:16]}"
    parts = []

    parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n{chat_id}")

    if reply_to:
        parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"reply_to_message_id\"\r\n\r\n{reply_to}")

    if emoji:
        parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"emoji\"\r\n\r\n{emoji}")

    text_body = "\r\n".join(parts) + "\r\n"

    filename = os.path.basename(file_path)
    file_header = (
        f"--{boundary}\r\n"
        f"Content-Disposition: form-data; name=\"sticker\"; filename=\"{filename}\"\r\n"
        f"Content-Type: application/octet-stream\r\n"
        f"\r\n"
    )

    with open(file_path, "rb") as f:
        file_data = f.read()

    closing = f"\r\n--{boundary}--\r\n"

    body = text_body.encode() + file_header.encode() + file_data + closing.encode()
    content_type = f"multipart/form-data; boundary={boundary}"

    return _send_via_socket("sendSticker", body, content_type)


def _is_local_file(s: str) -> bool:
    """Check if the string looks like a local file path rather than a file_id or URL."""
    if s.startswith("http://") or s.startswith("https://"):
        return False
    if os.path.exists(s):
        return True
    return False


def main():
    parser = argparse.ArgumentParser(description="Send a sticker via Telegram")
    parser.add_argument("--chat-id", "-c", required=True, help="Target chat ID")
    parser.add_argument("--sticker", "-s", required=True,
                        help="Sticker file_id, URL, or local file path (.webp/.tgs/.webm)")
    parser.add_argument("--reply-to", "-r", type=int, help="Message ID to reply to")
    parser.add_argument("--emoji", "-e", help="Associated emoji for the sticker")

    args = parser.parse_args()
    chat_id = args.chat_id.strip()

    try:
        if _is_local_file(args.sticker):
            result = send_sticker_file(chat_id, args.sticker, args.reply_to, args.emoji)
        else:
            # file_id or URL — both work via JSON API
            result = send_sticker_id(chat_id, args.sticker, args.reply_to, args.emoji)

        if not result.get("ok"):
            print(f"API error: {json.dumps(result)}", file=sys.stderr)
            sys.exit(1)

        _signal_idle(chat_id)
        print(f"Sticker sent to {chat_id}")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
