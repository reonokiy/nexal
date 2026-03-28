#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Send messages to Discord channels through the nexal token proxy."""

import argparse
import http.client
import json
import socket
import sys

PROXY_SOCK = "/workspace/agents/proxy/discord.com"


def _proxy_post(path: str, data: dict) -> dict:
    """POST to the Discord proxy. path is the Discord API path (e.g. /channels/123/messages)."""
    body = json.dumps(data, ensure_ascii=False).encode()
    conn = http.client.HTTPConnection("localhost")
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(PROXY_SOCK)
    conn.sock = sock
    try:
        conn.request("POST", path, body=body,
                      headers={"Content-Type": "application/json"})
        resp = conn.getresponse()
        result = json.loads(resp.read())
        if resp.status >= 400:
            print(f"API error ({resp.status}): {json.dumps(result)}", file=sys.stderr)
            sys.exit(1)
        return result
    finally:
        conn.close()


def main():
    parser = argparse.ArgumentParser(description="Send message to Discord")
    parser.add_argument("--channel", "-c", type=int, required=True, help="Channel ID")
    parser.add_argument("--message", "-m", required=True, help="Message to send")

    args = parser.parse_args()

    try:
        _proxy_post(f"/channels/{args.channel}/messages", {
            "content": args.message,
        })
        print(f"Message sent to channel {args.channel}")
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
