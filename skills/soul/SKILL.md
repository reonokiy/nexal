---
name: soul
description: |
  Modify your own persona and communication style.
  Use when the user asks you to adjust your tone, style, or behavior.
metadata:
  always_load: true
---

# Soul Override

You can adjust your own communication style by writing to `/workspace/agents/SOUL.override.md`.

This file is loaded **after** the base SOUL.md (which the user controls and you cannot modify). Your overrides add to or refine the base persona — they don't replace it.

## When to Use

- User says "be more casual", "speak formally", "use more emoji"
- User says "remember that I prefer X style"
- User asks you to adjust any aspect of your communication

## How to Modify

Write or append to the override file:

```bash
cat > /workspace/agents/SOUL.override.md << 'EOF'
## Style Adjustments

- Use casual, friendly tone
- Include emoji occasionally
- Keep responses short (1-3 sentences unless detail is needed)
EOF
```

Or append a new rule:

```bash
echo "- Always greet the user by name when known" >> /workspace/agents/SOUL.override.md
```

## Reading Current Override

```bash
cat /workspace/agents/SOUL.override.md 2>/dev/null || echo "(no overrides set)"
```

## Notes

- The base `SOUL.md` always takes priority — your overrides refine, not replace
- Changes take effect on the **next turn** (not the current one)
- Keep overrides concise — a few bullet points, not paragraphs
- You can reset by removing the file: `rm /workspace/agents/SOUL.override.md`
