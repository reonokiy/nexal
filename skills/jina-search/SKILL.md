---
name: jina-search
description: Search the web using Jina AI. Use when the user asks factual questions, recent events, or anything you need to look up.
metadata:
  always_load: true
---

# Web Search

Search the web via Jina AI API. Auth is injected automatically — no API key needed.

```bash
uv run /workspace/agents/skills/jina-search/scripts/search.py "your search query"
```

Options:
- `--limit N` — max results (default: 5)

Returns: title, URL, description, and content snippet for each result.
