#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Send a photo via Telegram through the nexal token proxy.

Supports sending a local file (multipart upload) or a URL.
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
    """Send BUSY->IDLE state transition signal via the state signal socket."""
    try:
        sig_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sig_sock.connect(STATE_SIGNAL_SOCK)
        payload = json.dumps({"session": f"telegram:{chat_id}", "state": "IDLE"})
        sig_sock.sendall((payload + "\n").encode())
        sig_sock.close()
    except Exception:
        pass


def _send_via_socket(method: str, body: bytes, content_type: str) -> dict:
    """Send raw HTTP request to the Telegram proxy over Unix socket."""
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

    # Read response
    chunks = []
    while True:
        chunk = sock.recv(8192)
        if not chunk:
            break
        chunks.append(chunk)
    sock.close()

    raw = b"".join(chunks)
    # Split headers from body
    parts = raw.split(b"\r\n\r\n", 1)
    if len(parts) == 2:
        resp_body = parts[1]
    else:
        resp_body = raw

    # Handle chunked transfer encoding
    header_str = parts[0].decode("utf-8", errors="replace") if len(parts) == 2 else ""
    if "transfer-encoding: chunked" in header_str.lower():
        resp_body = _decode_chunked(resp_body)

    return json.loads(resp_body)


def _decode_chunked(data: bytes) -> bytes:
    """Decode HTTP chunked transfer encoding."""
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
        buf.readline()  # trailing \r\n
    return bytes(result)


def send_photo_url(chat_id: str, photo_url: str, caption: str = "", reply_to: int | None = None) -> dict:
    """Send a photo by URL."""
    payload: dict = {
        "chat_id": chat_id,
        "photo": photo_url,
    }
    if caption:
        payload["caption"] = caption
    if reply_to:
        payload["reply_to_message_id"] = reply_to

    body = json.dumps(payload, ensure_ascii=False).encode()
    return _send_via_socket("sendPhoto", body, "application/json")


def send_photo_file(chat_id: str, file_path: str, caption: str = "", reply_to: int | None = None) -> dict:
    """Send a photo by uploading a local file (multipart/form-data)."""
    boundary = f"----FormBoundary{uuid.uuid4().hex[:16]}"
    parts = []

    # chat_id field
    parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n{chat_id}")

    # caption field
    if caption:
        parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"caption\"\r\n\r\n{caption}")

    # reply_to_message_id field
    if reply_to:
        parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"reply_to_message_id\"\r\n\r\n{reply_to}")

    # Build text parts
    text_body = "\r\n".join(parts) + "\r\n"

    # Photo file part header
    filename = os.path.basename(file_path)
    file_header = (
        f"--{boundary}\r\n"
        f"Content-Disposition: form-data; name=\"photo\"; filename=\"{filename}\"\r\n"
        f"Content-Type: application/octet-stream\r\n"
        f"\r\n"
    )

    # Read file
    with open(file_path, "rb") as f:
        file_data = f.read()

    # Closing boundary
    closing = f"\r\n--{boundary}--\r\n"

    body = text_body.encode() + file_header.encode() + file_data + closing.encode()
    content_type = f"multipart/form-data; boundary={boundary}"

    return _send_via_socket("sendPhoto", body, content_type)


def main():
    parser = argparse.ArgumentParser(description="Send a photo via Telegram")
    parser.add_argument("--chat-id", "-c", required=True, help="Target chat ID")
    parser.add_argument("--photo", "-p", required=True, help="Photo file path or URL")
    parser.add_argument("--caption", default="", help="Photo caption")
    parser.add_argument("--reply-to", "-r", type=int, help="Message ID to reply to")

    args = parser.parse_args()
    chat_id = args.chat_id.strip()

    try:
        if args.photo.startswith("http://") or args.photo.startswith("https://"):
            result = send_photo_url(chat_id, args.photo, args.caption, args.reply_to)
        else:
            result = send_photo_file(chat_id, args.photo, args.caption, args.reply_to)

        if not result.get("ok"):
            print(f"API error: {json.dumps(result)}", file=sys.stderr)
            sys.exit(1)

        _signal_idle(chat_id)
        print(f"Photo sent to {chat_id}")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
