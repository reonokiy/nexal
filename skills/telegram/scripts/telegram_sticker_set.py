#!/usr/bin/env uv run
# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Browse and manage Telegram sticker sets.

Subcommands:
  list              — Show all configured sticker sets
  browse <set_name> — Fetch a sticker set and show all stickers (emoji + file_id)
  add <set_name>    — Add a sticker set to the configured list
  remove <set_name> — Remove a sticker set from the configured list
  pick <set_name> <query> — Find stickers matching an emoji or keyword

The configured sets are stored in /workspace/agents/config/sticker_sets.txt (one set name per line).
Fetched sticker data is cached in /workspace/agents/config/sticker_cache/ as JSON.
"""

import argparse
import http.client
import json
import os
import socket
import sys
from pathlib import Path

PROXY_SOCK = "/workspace/agents/proxy/api.telegram.org"
CONFIG_DIR = Path("/workspace/agents/config")
SETS_FILE = CONFIG_DIR / "sticker_sets.txt"
CACHE_DIR = CONFIG_DIR / "sticker_cache"


def _proxy_post(method: str, data: dict) -> dict:
    body = json.dumps(data, ensure_ascii=False).encode()
    conn = http.client.HTTPConnection("localhost")
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(PROXY_SOCK)
    conn.sock = sock
    try:
        conn.request("POST", f"/{method}", body=body,
                      headers={"Content-Type": "application/json"})
        resp = conn.getresponse()
        result = json.loads(resp.read())
        return result
    finally:
        conn.close()


def _ensure_dirs():
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    if not SETS_FILE.exists():
        SETS_FILE.touch()


def _read_configured_sets() -> list[str]:
    _ensure_dirs()
    lines = SETS_FILE.read_text().strip().splitlines()
    return [l.strip() for l in lines if l.strip() and not l.strip().startswith("#")]


def _write_configured_sets(sets: list[str]):
    _ensure_dirs()
    SETS_FILE.write_text("\n".join(sets) + "\n")


def _fetch_sticker_set(set_name: str) -> dict | None:
    """Fetch sticker set from Telegram API and cache it."""
    result = _proxy_post("getStickerSet", {"name": set_name})
    if not result.get("ok"):
        print(f"Error fetching sticker set '{set_name}': {json.dumps(result)}", file=sys.stderr)
        return None

    sticker_set = result["result"]
    # Build compact cache: list of {emoji, file_id, index, type}
    stickers = []
    for i, s in enumerate(sticker_set.get("stickers", [])):
        stickers.append({
            "index": i,
            "emoji": s.get("emoji", ""),
            "file_id": s.get("file_id", ""),
            "type": s.get("type", "regular"),
            "custom_emoji_id": s.get("custom_emoji_id", ""),
        })

    cache_data = {
        "name": sticker_set.get("name", set_name),
        "title": sticker_set.get("title", ""),
        "sticker_type": sticker_set.get("sticker_type", ""),
        "count": len(stickers),
        "stickers": stickers,
    }

    # Write cache
    _ensure_dirs()
    cache_path = CACHE_DIR / f"{set_name}.json"
    cache_path.write_text(json.dumps(cache_data, ensure_ascii=False, indent=2))

    return cache_data


def _load_cached(set_name: str) -> dict | None:
    cache_path = CACHE_DIR / f"{set_name}.json"
    if cache_path.exists():
        return json.loads(cache_path.read_text())
    return None


def _format_sticker_table(data: dict) -> str:
    """Format sticker set as a compact table for the model to read."""
    lines = [f"📦 {data['title']} ({data['name']}) — {data['count']} stickers\n"]
    lines.append(f"{'#':<4} {'Emoji':<8} {'file_id'}")
    lines.append("-" * 60)
    for s in data["stickers"]:
        lines.append(f"{s['index']:<4} {s['emoji']:<8} {s['file_id']}")
    return "\n".join(lines)


def cmd_list(_args):
    sets = _read_configured_sets()
    if not sets:
        print("No sticker sets configured.")
        print("Use 'add <set_name>' to add one.")
        print("Tip: send a sticker in chat — the set_name appears in the sticker metadata.")
        return

    print(f"Configured sticker sets ({len(sets)}):\n")
    for name in sets:
        cached = _load_cached(name)
        if cached:
            print(f"  • {name} — \"{cached['title']}\" ({cached['count']} stickers)")
        else:
            print(f"  • {name} (not fetched yet, use 'browse {name}')")


def cmd_browse(args):
    set_name = args.set_name
    use_cache = not args.refresh

    data = None
    if use_cache:
        data = _load_cached(set_name)

    if not data:
        data = _fetch_sticker_set(set_name)

    if not data:
        sys.exit(1)

    print(_format_sticker_table(data))


def cmd_add(args):
    sets = _read_configured_sets()
    if args.set_name in sets:
        print(f"'{args.set_name}' is already configured.")
        return
    sets.append(args.set_name)
    _write_configured_sets(sets)
    print(f"Added '{args.set_name}'. Use 'browse {args.set_name}' to fetch stickers.")

    # Auto-fetch
    _fetch_sticker_set(args.set_name)


def cmd_remove(args):
    sets = _read_configured_sets()
    if args.set_name not in sets:
        print(f"'{args.set_name}' is not in configured sets.")
        return
    sets.remove(args.set_name)
    _write_configured_sets(sets)
    # Remove cache
    cache_path = CACHE_DIR / f"{args.set_name}.json"
    if cache_path.exists():
        cache_path.unlink()
    print(f"Removed '{args.set_name}'.")


def cmd_pick(args):
    set_name = args.set_name
    query = args.query.lower()

    data = _load_cached(set_name)
    if not data:
        data = _fetch_sticker_set(set_name)
    if not data:
        sys.exit(1)

    matches = []
    for s in data["stickers"]:
        if query in s["emoji"].lower() or query == str(s["index"]):
            matches.append(s)

    if not matches:
        print(f"No stickers matching '{args.query}' in {set_name}.")
        print("Try browsing the full set to see available emojis.")
        sys.exit(1)

    if len(matches) == 1:
        print(matches[0]["file_id"])
    else:
        print(f"Found {len(matches)} matches:\n")
        for s in matches:
            print(f"  #{s['index']}  {s['emoji']}  {s['file_id']}")


def main():
    parser = argparse.ArgumentParser(description="Browse and manage Telegram sticker sets")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("list", help="Show all configured sticker sets")

    p_browse = sub.add_parser("browse", help="Fetch and display a sticker set")
    p_browse.add_argument("set_name", help="Sticker set name")
    p_browse.add_argument("--refresh", action="store_true", help="Force re-fetch from Telegram")

    p_add = sub.add_parser("add", help="Add a sticker set to configured list")
    p_add.add_argument("set_name", help="Sticker set name")

    p_remove = sub.add_parser("remove", help="Remove a sticker set")
    p_remove.add_argument("set_name", help="Sticker set name")

    p_pick = sub.add_parser("pick", help="Find stickers matching emoji or index")
    p_pick.add_argument("set_name", help="Sticker set name")
    p_pick.add_argument("query", help="Emoji or sticker index to find")

    args = parser.parse_args()
    {"list": cmd_list, "browse": cmd_browse, "add": cmd_add, "remove": cmd_remove, "pick": cmd_pick}[args.command](args)


if __name__ == "__main__":
    main()
