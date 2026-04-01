---
name: jina-reader
description: Read and extract content from any URL as clean markdown. Use when the user shares a link or you need to read a web page.
metadata:
  always_load: true
---

# Web Page Reader

Read and extract content from any URL via Jina AI Reader API. Auth is injected automatically — no API key needed.

```bash
uv run /workspace/agents/skills/jina-reader/scripts/read.py "https://example.com"
```

Returns: page title, description, and full content as clean markdown.
