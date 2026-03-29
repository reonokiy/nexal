---
name: proxy
description: API token proxy — access external APIs without tokens
metadata:
  always_load: true
---

# API Token Proxy

Authentication tokens are injected on the host side. You can access APIs through Unix socket proxies at `/workspace/agents/proxy/`. **You never need API tokens** — the proxy handles authentication automatically.

## Available Proxies

| API | Socket Path | Upstream |
|-----|-------------|----------|
| Telegram Bot API | `/workspace/agents/proxy/api.telegram.org` | `https://api.telegram.org/bot<TOKEN>/` |
| Discord API | `/workspace/agents/proxy/discord.com` | `https://discord.com/api/` |

## How to Use

Replace the API base URL with the Unix socket path. The proxy injects the auth token and forwards your request.

### Quick Example (curl)

```bash
# Send a Telegram message (no token needed!)
curl --unix-socket /workspace/agents/proxy/api.telegram.org \
  -X POST http://localhost/sendMessage \
  -H "Content-Type: application/json" \
  -d '{"chat_id": "123456", "text": "Hello from nexal!"}'
```

### Python Example (single file, uv-runnable)

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///

import httpx
import sys

PROXY_SOCK = "/workspace/agents/proxy/api.telegram.org"

def send_message(chat_id: str, text: str):
    transport = httpx.HTTPTransport(uds=PROXY_SOCK)
    with httpx.Client(transport=transport, base_url="http://localhost") as client:
        resp = client.post("/sendMessage", json={"chat_id": chat_id, "text": text})
        return resp.json()

if __name__ == "__main__":
    chat_id = sys.argv[1]
    text = " ".join(sys.argv[2:])
    result = send_message(chat_id, text)
    print(result)
```

Run with: `uv run script.py <chat_id> <text>`

## Writing Custom Scripts

When existing skills don't cover your needs, you can write your own single-file Python scripts:

1. **Use `uv run` inline metadata** — dependencies in the file header, no venv setup needed
2. **Connect via Unix socket** — use `httpx` with `HTTPTransport(uds=PROXY_SOCK)`
3. **API paths map directly** — just use the API's path (e.g. `/sendMessage` for Telegram)
4. **No tokens needed** — the proxy injects authentication automatically

### Template

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///

import httpx
import json
import sys

# Choose the right proxy socket for your API
PROXY_SOCK = "/workspace/agents/proxy/api.telegram.org"
# PROXY_SOCK = "/workspace/agents/proxy/discord.com"

transport = httpx.HTTPTransport(uds=PROXY_SOCK)
client = httpx.Client(transport=transport, base_url="http://localhost")

# Make your API call
response = client.post("/your-api-method", json={"key": "value"})
print(json.dumps(response.json(), indent=2))
```

## Notes

- Proxies are HTTP/1.1 over Unix sockets
- Only POST and GET methods are supported
- Response is JSON from the upstream API
- Socket files are at `/workspace/agents/proxy/` (read-write)
