#!/usr/bin/env python3
"""Query tool call records via the nexal DB API."""

import argparse
import http.client
import json
import socket
import sys

API_SOCK = "/workspace/agents/proxy/nexal-api"


def api_call(endpoint: str, params: dict | None = None) -> list | dict:
    body = json.dumps(params or {}).encode()
    conn = http.client.HTTPConnection("localhost")
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(API_SOCK)
    conn.sock = sock
    try:
        conn.request("POST", endpoint, body=body,
                      headers={"Content-Type": "application/json"})
        resp = conn.getresponse()
        result = json.loads(resp.read())
    finally:
        conn.close()
    if isinstance(result, dict) and "error" in result:
        print(json.dumps(result))
        sys.exit(1)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description="Query tool call records")
    parser.add_argument("--channel", help="Filter by channel (e.g. telegram, discord)")
    parser.add_argument("--chat-id", help="Filter by chat/conversation ID")
    parser.add_argument("--tool-name", help="Filter by tool name")
    parser.add_argument("--status", choices=["ok", "error"], help="Filter by status")
    parser.add_argument("--since", help="Timestamp lower bound")
    parser.add_argument("--until", help="Timestamp upper bound")
    parser.add_argument("--limit", type=int, default=50, help="Max rows (default: 50)")
    parser.add_argument("--offset", type=int, default=0, help="Skip first N results")
    args = parser.parse_args()

    params = {"limit": args.limit, "offset": args.offset}
    if args.channel: params["channel"] = args.channel
    if args.chat_id: params["chat_id"] = args.chat_id
    if args.tool_name: params["tool_name"] = args.tool_name
    if args.status: params["status"] = args.status
    if args.since: params["since"] = args.since
    if args.until: params["until"] = args.until

    results = api_call("/toollog/query", params)
    print(json.dumps(results, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
