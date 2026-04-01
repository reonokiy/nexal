#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///

"""Search the web using Jina AI Search API (s.jina.ai) via the nexal proxy."""

import httpx
import sys
import urllib.parse

PROXY = "/workspace/agents/proxy/s.jina.ai"


def search(query: str, limit: int = 5) -> None:
    transport = httpx.HTTPTransport(uds=PROXY)
    client = httpx.Client(transport=transport, base_url="http://localhost", timeout=30)

    # Jina Search: GET /<query>
    encoded_query = urllib.parse.quote(query)
    resp = client.get(f"/{encoded_query}")

    if resp.status_code != 200:
        print(f"Error: {resp.status_code} {resp.text[:300]}")
        sys.exit(1)

    data = resp.json()
    results = data.get("data", [])

    if not results:
        print("No results found.")
        return

    for i, result in enumerate(results[:limit], 1):
        title = result.get("title", "Untitled")
        url = result.get("url", "")
        description = result.get("description", "")
        content = result.get("content", "")

        print(f"## {i}. {title}")
        print(f"URL: {url}")
        if description:
            print(f"{description}")
        if content:
            text = content[:500]
            if len(content) > 500:
                text += "..."
            print(f"\n{text}")
        print()


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: search.py <query> [--limit N]")
        sys.exit(1)

    query_parts = []
    limit = 5
    i = 1
    while i < len(sys.argv):
        if sys.argv[i] == "--limit" and i + 1 < len(sys.argv):
            limit = int(sys.argv[i + 1])
            i += 2
        else:
            query_parts.append(sys.argv[i])
            i += 1

    search(" ".join(query_parts), limit)
