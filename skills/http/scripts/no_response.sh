#!/bin/sh
# Explicit signal that the agent chose not to respond to this message.
# Usage: ./no_response.sh --chat-id <CHAT_ID>

CHAT_ID=""
while [ $# -gt 0 ]; do
    case "$1" in
        --chat-id|-c) CHAT_ID="$2"; shift 2 ;;
        *) shift ;;
    esac
done

if [ -z "$CHAT_ID" ]; then
    echo "Error: --chat-id is required" >&2
    exit 1
fi

SIGNAL_SOCK="/workspace/agents/.state"
if [ -S "$SIGNAL_SOCK" ]; then
    printf '{"session":"http:%s","state":"IDLE"}\n' "$CHAT_ID" | \
        socat - UNIX-CONNECT:"$SIGNAL_SOCK" 2>/dev/null || true
fi

echo "ok: no response"
