---
name: skill-manager
description: |
  Create, install, list, and manage custom skills (admin only).
metadata:
  always_load: true
  admin_only: true
---

# Skill Manager

Skills live in two directories inside the container:

| Directory | Access | Purpose |
|-----------|--------|---------|
| `/workspace/agents/skills/` | Read-only | Built-in skills shipped with nexal |
| `/workspace/agents/skills.override/` | Read-write | Your custom skills |

Custom skills in `skills.override/` with the same name as built-in ones **replace** them.

## Creating a Skill

Create a directory in `skills.override/` with a `SKILL.md`:

```bash
mkdir -p /workspace/agents/skills.override/my-skill
cat > /workspace/agents/skills.override/my-skill/SKILL.md << 'SKILLEOF'
---
name: my-skill
description: What this skill does.
metadata:
  always_load: true
---

# My Skill

Instructions for using this skill...
SKILLEOF
```

### With Scripts

```bash
mkdir -p /workspace/agents/skills.override/my-skill/scripts

cat > /workspace/agents/skills.override/my-skill/scripts/run.py << 'PYEOF'
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///
import sys
print("Hello from my skill!", sys.argv[1:])
PYEOF

chmod +x /workspace/agents/skills.override/my-skill/scripts/run.py
```

## Installing from GitHub

Download a skill from any GitHub repo into `skills.override/`:

```bash
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.10"
# dependencies = ["httpx"]
# ///
import httpx, sys, json, base64, os

repo = sys.argv[1]      # e.g. "owner/repo"
path = sys.argv[2]      # e.g. "skills/my-skill"
name = path.rstrip("/").split("/")[-1]
dest = f"/workspace/agents/skills.override/{name}"
ref = sys.argv[3] if len(sys.argv) > 3 else "main"

api = f"https://api.github.com/repos/{repo}/contents/{path}?ref={ref}"
items = httpx.get(api).json()

os.makedirs(dest, exist_ok=True)

for item in items:
    if item["type"] == "file":
        content = httpx.get(item["download_url"]).text
        filepath = os.path.join(dest, item["name"])
        with open(filepath, "w") as f:
            f.write(content)
        print(f"  {item['name']}")
    elif item["type"] == "dir":
        # Recurse for subdirectories (scripts/, references/)
        sub_api = f"https://api.github.com/repos/{repo}/contents/{path}/{item['name']}?ref={ref}"
        sub_items = httpx.get(sub_api).json()
        sub_dir = os.path.join(dest, item["name"])
        os.makedirs(sub_dir, exist_ok=True)
        for sub in sub_items:
            if sub["type"] == "file":
                content = httpx.get(sub["download_url"]).text
                filepath = os.path.join(sub_dir, sub["name"])
                with open(filepath, "w") as f:
                    f.write(content)
                print(f"  {item['name']}/{sub['name']}")

print(f"\nInstalled '{name}' to {dest}")
print("Skill will be available on next turn.")
```

Usage: `uv run install.py owner/repo path/to/skill [ref]`

## Listing Skills

```bash
echo "=== Built-in skills ==="
ls /workspace/agents/skills/ 2>/dev/null

echo ""
echo "=== Custom skills ==="
ls /workspace/agents/skills.override/ 2>/dev/null || echo "(none)"
```

## Removing a Custom Skill

```bash
rm -rf /workspace/agents/skills.override/<skill-name>
```

Only custom skills can be removed. Built-in skills are read-only.

## Skill Structure

```
my-skill/
├── SKILL.md          ← Required. Frontmatter + instructions.
├── scripts/          ← Optional. Executable scripts.
│   └── run.py
└── references/       ← Optional. Extra docs loaded on demand.
    └── api.md
```

### SKILL.md Frontmatter

```yaml
---
name: my-skill
description: One-line description shown in skill list.
metadata:
  always_load: true     # Load for all channels (optional)
  channel: telegram     # Load only for this channel (optional)
---
```
