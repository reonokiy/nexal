---
name: cli
description: |
  CLI channel — interactive terminal session.
  exec stdout is displayed to the user. Text responses are also displayed.
metadata:
  channel: cli
---

# CLI Skill

The CLI channel is an interactive terminal session.

## How to respond

You have two ways to send messages:

1. **exec** — anything printed to stdout by exec is displayed to the user immediately. Use this for rich or programmatic output:
   ```bash
   # Simple text
   echo "hello"

   # Rich output with Python
   python3 -c "
   for name, score in [('Alice', 95), ('Bob', 82)]:
       print(f'{name:<10} {score:>5}')
   "
   ```

2. **Text response** — when you stop calling tools, your final text is also displayed. Use this for short conversational replies.

## Sending multiple messages

Call exec multiple times to send several messages, like texting:
```
exec: echo "hmm let me check"
exec: python3 -c "print('ok found it — ...')"
```

This feels more natural than one huge block of text.
