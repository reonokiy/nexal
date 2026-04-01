#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///

"""Read and extract content from a URL using Jina AI Reader API (r.jina.ai) via the nexal proxy."""

import httpx
import sys
import urllib.parse

PROXY = "/workspace/agents/proxy/r.jina.ai"


def read_url(url: str) -> None:
    transport = httpx.HTTPTransport(uds=PROXY)
    client = httpx.Client(transport=transport, base_url="http://localhost", timeout=60)

    # Jina Reader: GET /<url>
    encoded_url = urllib.parse.quote(url, safe="")
    resp = client.get(f"/{encoded_url}")

    if resp.status_code != 200:
        print(f"Error: {resp.status_code} {resp.text[:300]}")
        sys.exit(1)

    data = resp.json()
    result = data.get("data", {})

    title = result.get("title", "Untitled")
    content = result.get("content", "")
    description = result.get("description", "")

    print(f"# {title}")
    if description:
        print(f"\n> {description}")
    print(f"\n{content}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: read.py <url>")
        sys.exit(1)

    read_url(sys.argv[1])
