#!/usr/bin/env python3
"""Send responses via the HTTP channel response socket."""

import argparse
import http.client
import json
import socket
import sys

RESPONSE_SOCK = "/workspace/agents/proxy/http.channel"
STATE_SIGNAL_SOCK = "/workspace/agents/.state"


def _signal_idle(chat_id: str):
    """Send BUSY→IDLE state transition signal via the state signal socket."""
    try:
        sig_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sig_sock.connect(STATE_SIGNAL_SOCK)
        payload = json.dumps({"session": f"http:{chat_id}", "state": "IDLE"})
        sig_sock.sendall((payload + "\n").encode())
        sig_sock.close()
    except Exception:
        pass


def send_message(chat_id: str, text: str) -> dict:
    body = json.dumps({"chat_id": chat_id, "text": text}, ensure_ascii=False).encode()
    conn = http.client.HTTPConnection("localhost")
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(RESPONSE_SOCK)
    conn.sock = sock
    try:
        conn.request(
            "POST",
            "/response",
            body=body,
            headers={"Content-Type": "application/json"},
        )
        resp = conn.getresponse()
        result = json.loads(resp.read())
        if resp.status >= 400:
            print(f"Error ({resp.status}): {json.dumps(result)}", file=sys.stderr)
            sys.exit(1)
        return result
    finally:
        conn.close()


def main():
    parser = argparse.ArgumentParser(description="Send HTTP channel response")
    parser.add_argument("--chat-id", "-c", required=True, help="Target chat ID")
    parser.add_argument("--message", "-m", required=True, help="Message text")

    args = parser.parse_args()
    chat_id = args.chat_id.strip()

    try:
        send_message(chat_id, args.message)
        _signal_idle(chat_id)
        print(f"Response sent to {chat_id}")
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
