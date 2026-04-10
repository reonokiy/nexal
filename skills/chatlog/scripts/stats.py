#!/usr/bin/env python3
"""Get message statistics via the nexal DB API."""

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
    result = api_call("/chatlog/stats")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
