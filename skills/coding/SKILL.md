---
name: coding
description: Coding conventions for writing scripts inside the sandbox.
metadata:
  always_load: true
---

# Coding Conventions

## Python Scripts

All Python scripts must be **single-file** with inline metadata per [PEP 723](https://peps.python.org/pep-0723/).

### Format

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "httpx",
#     "rich",
# ]
# ///

import httpx
# ... your code ...
```

### Rules

1. **Always use PEP 723 inline metadata** — declare `requires-python` and `dependencies` in the file header
2. **Run with `uv run`** — never `pip install`, never create venvs
3. **Single file** — no packages, no setup.py, no pyproject.toml for scripts
4. **`#!/usr/bin/env -S uv run`** shebang — makes the file directly executable

### Running

```bash
# Direct execution (preferred)
uv run script.py

# Or make executable and run
chmod +x script.py
./script.py
```

### Example

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "requests",
# ]
# ///

import requests
import sys

url = sys.argv[1] if len(sys.argv) > 1 else "https://httpbin.org/get"
resp = requests.get(url)
print(resp.json())
```
