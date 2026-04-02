#!/bin/sh
# Explicit signal that the agent has nothing to do on this heartbeat.

SIGNAL_SOCK="/workspace/agents/.state"
if [ -S "$SIGNAL_SOCK" ]; then
    printf '{"session":"heartbeat:main","state":"IDLE"}\n' | \
        socat - UNIX-CONNECT:"$SIGNAL_SOCK" 2>/dev/null || true
fi

echo "ok: no response"
