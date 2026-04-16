# nexal

nexal's multi-channel bot runtime, rewritten in TypeScript on top of
[`pi-agent-core`](https://github.com/badlogic/pi-mono/tree/main/packages/agent)
and [`pi-ai`](https://github.com/badlogic/pi-mono/tree/main/packages/ai).

## Layout

```
src/
  index.ts            — entry: load config, start channels, run forever
  agent-pool.ts       — one pi-agent-core Agent per (chat_id); lifecycle + debounce
  channels/
    types.ts          — Channel interface, IncomingMessage, OutgoingReply
    telegram.ts       — Telegram channel (mirrors crates/channel-telegram)
    http.ts           — HTTP test channel (mirrors crates/channel-http)
    heartbeat.ts      — Periodic tick (mirrors crates/channel-heartbeat)
    cron.ts           — Agent-scheduled cron (mirrors crates/channel-cron)
  tools/
    bash.ts           — bash tool proxied to nexal-agent over WebSocket
    (read/write/edit — likely added as AgentTool definitions)
  exec-client.ts      — WebSocket client for crates/nexal-agent
  config.ts           — TOML + env config loader
```

## Design

- Each incoming message is keyed by channel+chat_id → routed to an `Agent`
  in the pool. If none exists, a new one is constructed with the session
  system prompt + any persisted messages.
- `Agent.prompt(userMsg)` drives one turn; tool calls inside the turn hit
  the bash tool, which opens a WebSocket to a per-session `nexal-agent`
  instance (running inside a sandbox container).
- Mid-turn messages (same chat, user typed again) are injected via
  `agent.steer(...)` so the model sees them on the next LLM hop.
- Replies flow back out via `Channel.send`.

## Runtime

- Bun ≥ 1.3 (for native TS + WebSocket client)
- `crates/nexal-agent` must be built (default path `target/release/nexal-agent`)

## Status

Scaffolding. Not functional yet.
