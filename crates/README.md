# nexal crates

## Two-tier architecture

The workspace is split into two tiers:

### Tier 1 — nexal-own crates (~5,600 LOC)

These crates are written specifically for nexal. They own the product
layer: channel adapters, config, agent orchestration, and the binary.

| Crate | Package | Purpose |
|-------|---------|---------|
| `nexal` | `nexal` | Main binary — wires channels, TUI, and agent together |
| `nexal-config` | `nexal-config` | Top-level config loaded from TOML + env |
| `agent` | `nexal-agent` | Bot orchestrator — debounce, routing, agent pool |
| `nexal-state` | `nexal-state` | SQLite state DB (cron jobs, chat log) |
| `channel-core` | `nexal-channel-core` | `Channel` trait, `IncomingMessage`, debounce |
| `channel-telegram` | `nexal-channel-telegram` | Telegram adapter (teloxide) |
| `channel-discord` | `nexal-channel-discord` | Discord adapter (serenity) |
| `channel-http` | `nexal-channel-http` | HTTP test adapter (axum) |
| `channel-heartbeat` | `nexal-channel-heartbeat` | Periodic heartbeat tick |
| `channel-cron` | `nexal-channel-cron` | Agent-scheduled cron jobs |

### Tier 2 — Forked Codex engine (~370K LOC)

These crates are forked from [codex-rs](https://github.com/openai/codex-rs)
at commit `315e7d6`. All types were renamed from `codex → nexal` at fork
time. They implement the session engine, TUI shell, exec sandbox, MCP
client, and tool system.

Key crates in this tier: `core`, `tui`, `tui-render`, `app-server`,
`protocol`, `exec-server`, `rmcp-client`, and ~30 utility crates under
`utils/`.

#### Important: `crates/core/src/nexal.rs`

This file is **not** nexal-product code. It is the session engine that
was originally `crates/core/src/codex.rs` in the upstream repo — renamed
to `nexal.rs` at fork commit `315e7d6`. The `nexal` struct is the upstream
`Codex` struct under a different name. Do not add nexal-product logic here.

## Crate boundaries

- **Tier 1 → Tier 2**: Allowed. nexal channels use `nexal-core` types
  (e.g. `Config`) only through `nexal-agent`, which uses
  `nexal-app-server-client` for in-process RPC.
- **Tier 2 → Tier 1**: Not allowed. Forked crates must not import
  channel, heartbeat, cron, or product-specific modules.
- **Config fan-in**: Each channel crate owns its own typed config struct
  (e.g. `TelegramChannelConfig` lives in `channel-telegram/src/config.rs`).
  `nexal-config` stores a raw `HashMap<String, toml::Value>` for channel
  config so touching one channel's config doesn't rebuild all channels.
