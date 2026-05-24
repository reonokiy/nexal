# Nexal Project Structure

> Last updated: 2026-05-23

## Overview

Nexal is a multi-channel AI agent runtime with four main components:

1. **LLM Server** (`src/`) — Bun/TypeScript daemon that hosts agents, channels, commands, workers, and tools.
2. **Gateway** (`crates/nexal-gateway/`) — Rust WebSocket gateway that authenticates clients and manages sandbox backends.
3. **Agent** (`crates/nexal-agent/`) — Rust sandbox executor exposing bash, filesystem, and process JSON-RPC tools.
4. **Web UI** (`web/`) — Svelte 5 frontend that connects to the LLM server over WebSocket.

## Architecture

```text
Input Channels         Core (LLM Server)             Gateway                  Sandbox Agent
├─ Web UI              ├─ Channel Manager            ├─ HMAC Auth             ├─ JSON-RPC Server
├─ WebSocket           ├─ Agent Pool                 ├─ WebSocket Proxy       ├─ Bash/Exec
├─ HTTP                ├─ Worker Registry            ├─ Session Registry      ├─ File System
├─ Telegram            ├─ Slash Commands             ├─ Backends              └─ Process Manager
├─ GitHub              ├─ Tools                      │  ├─ Fly.io Machines
├─ Cron                │  ├─ bash                    │  ├─ Podman
└─ Heartbeat           │  ├─ coordinator             │  └─ Kubernetes
                       │  ├─ send_update
                       │  └─ report_to_parent
                       └─ Gateway Client
```

Data flow:

```text
User Message
  -> Channel
  -> ChannelManager
  -> AgentPool
  -> pi-agent-core Agent
  -> LLM
  -> Tool Call
  -> GatewayClient
  -> nexal-gateway
  -> nexal-agent in sandbox
```

## Directory Layout

```text
nexal/
├── src/                          # TypeScript LLM server
│   ├── cli.ts                    # CLI entry point used by package scripts/bin
│   ├── index.ts                  # Startup implementation
│   ├── agent-pool.ts             # Agent lifecycle and per-chat sessions
│   ├── auth.ts                   # Supabase/JWT auth helpers
│   ├── config.ts                 # Defaults, TOML, NEXAL_* config overlay
│   ├── db.ts                     # Postgres/Drizzle connection
│   ├── schema.ts                 # Schema barrel
│   ├── settings.ts               # DB-backed settings KV
│   ├── channels/                 # Channel implementations
│   │   ├── factory.ts
│   │   ├── manager.ts
│   │   ├── ws.ts
│   │   ├── http.ts
│   │   ├── github.ts
│   │   ├── heartbeat.ts
│   │   ├── cron.ts
│   │   └── telegram/
│   │       ├── channel.ts
│   │       ├── api.ts
│   │       └── types.ts
│   ├── commands/
│   │   ├── builtin.ts            # /help, /model, /apikey, /providers, /status, /sandboxes, /settings
│   │   └── registry.ts
│   ├── gateway/
│   │   ├── client.ts             # HMAC-signed gateway client
│   │   ├── agent_client.ts       # Per-session agent client
│   │   ├── protocol.ts
│   │   ├── transport.ts
│   │   └── errors.ts
│   ├── tools/
│   │   ├── bash.ts
│   │   ├── send_update.ts
│   │   ├── report_to_parent.ts
│   │   └── coordinator/
│   │       ├── index.ts
│   │       ├── spawn.ts
│   │       └── manage.ts
│   ├── workers/                  # Persistent worker subsystem
│   ├── tape/                     # Server-side tape handling
│   ├── prompts/
│   ├── scripts/
│   └── e2e/
│
├── crates/                       # Rust workspace
│   ├── nexal-agent/              # Sandbox JSON-RPC agent
│   │   └── src/
│   │       ├── bin/nexal-agent.rs
│   │       ├── server.rs
│   │       ├── server/
│   │       ├── protocol/
│   │       ├── process/
│   │       ├── fs/
│   │       └── proxy.rs
│   ├── nexal-gateway/            # Gateway server
│   │   └── src/
│   │       ├── bin/nexal-gateway.rs
│   │       ├── server.rs
│   │       ├── config.rs
│   │       ├── backend/
│   │       ├── proxy/
│   │       └── protocol.rs
│   └── utils/                    # absolute-path, cargo-bin, certs, json-transport, pty
│
├── web/                          # Svelte 5 Web UI
│   └── src/
│       ├── main.ts
│       ├── App.svelte
│       └── lib/
│           ├── client.svelte.ts
│           ├── settings.svelte.ts
│           ├── supabase.svelte.ts
│           ├── router.svelte.ts
│           ├── components/
│           │   ├── auth-form.svelte
│           │   ├── composer.svelte
│           │   ├── computers-page.svelte
│           │   ├── message.svelte
│           │   ├── sidebar.svelte
│           │   └── settings/
│           └── views/chat-view.svelte
│
├── packages/
│   ├── chat-client/              # MsgPack WebSocket chat client
│   └── tape/                     # Shared tape interfaces/types
│
├── skills/                       # Built-in agent skills served to sandboxes
│   ├── chatlog/
│   ├── cli/
│   ├── coding/
│   ├── cron/
│   ├── discord/
│   ├── heartbeat/
│   ├── http/
│   ├── jina-reader/
│   ├── jina-search/
│   ├── skill-manager/
│   ├── soul/
│   ├── telegram/
│   └── toollog/
│
├── deploy/                       # Per-service Fly configs + Dockerfiles
│   ├── server/{Dockerfile,fly.toml}
│   ├── gateway/{Dockerfile,fly.toml,entrypoint.sh}
│   ├── agent/Dockerfile
│   └── sandbox/Dockerfile
├── drizzle/                      # Drizzle migrations
├── .github/workflows/            # Deploy, build, Docker, release workflows
├── package.json
├── Cargo.toml
└── justfile
```

## Key Technologies

| Layer | Stack |
|-------|-------|
| LLM Server | Bun >= 1.3, TypeScript, `pi-agent-core`, `pi-ai`, Drizzle ORM |
| Gateway | Rust, Tokio, Axum, WebSocket, HMAC auth |
| Agent | Rust, JSON-RPC, WebSocket, `portable-pty` |
| Web UI | Svelte 5, Tailwind CSS 4, Vite |
| Database | Postgres/Neon |
| Auth | Supabase Auth/JWT |
| Deployment | Fly.io, GitHub Actions |

## Configuration

The Bun server loads structural config from these sources, lowest to highest priority:

1. Built-in defaults
2. `~/.nexal/config.toml` or `NEXAL_CONFIG_PATH`
3. Environment variables prefixed with `NEXAL_`, using `__` as the nesting separator

Important runtime values include:

| Variable | Purpose |
|----------|---------|
| `DATABASE_URL` | Postgres connection string |
| `NEXAL_GATEWAY_ACCESS_KEY` | Gateway credential ID |
| `NEXAL_GATEWAY_SECRET_KEY` | Gateway credential secret for HMAC signing |
| `SUPABASE_JWT_SECRET` / Supabase env | Auth integration, depending on deployment |
| `STORAGE_S3_*` or `NEXAL_STORAGE__*` | Object storage settings |

Model, provider, auth provider, channel, and tool API key configuration is primarily stored in Postgres settings KV. Relevant keys include `model:provider`, `model:id`, `provider:<name>`, `auth:<provider>`, `channel:<name>`, and `tool:apikey:<name>`. Manage these from the Web UI or slash commands such as `/model`, `/apikey`, `/providers`, and `/settings`.

The gateway has its own Rust config path and CLI/env handling. It commonly reads `~/.nexal/gateway.toml` plus CLI and environment values.

## Development

```bash
# LLM server with auto-reload
bun run dev

# TypeScript checks and tests
bun run typecheck
bun test

# Web UI
cd web && bun run dev
cd web && bun run check
cd web && bun run build

# Rust
cargo check
cargo test
cargo build --release

# Aggregated wrappers
just check
just test
```

## Web UI Defaults

The Web UI defaults to `wss://api.nexal.nokiy.net` as the backend URL. Users can override it with `VITE_NEXAL_BACKEND` or from the Settings page. Browser-side settings are stored in `localStorage` with the `nexal.` prefix.

## Deployment

- **Gateway**: `deploy/gateway/{Dockerfile,fly.toml,entrypoint.sh}`, `.github/workflows/deploy-gateway.yml`.
- **LLM Server**: `deploy/server/{Dockerfile,fly.toml}`, `.github/workflows/deploy-server.yml`.
- **Web UI**: `.github/workflows/deploy-web-pages.yml`, `.github/workflows/web-build.yml`.
- **Sandbox images**: `deploy/sandbox/Dockerfile`, `.github/workflows/docker.yml`.
- **Agent image**: `deploy/agent/Dockerfile`, `.github/workflows/release.yml`.

## Reference Files

- Root scripts: `package.json`, `justfile`
- Server config/settings: `src/config.ts`, `src/settings.ts`, `src/index.ts`
- Channel registry: `src/channels/manager.ts`, `src/channels/factory.ts`
- Gateway backends: `crates/nexal-gateway/src/backend/`
- Agent protocol/server: `crates/nexal-agent/src/protocol/`, `crates/nexal-agent/src/server.rs`
- Web settings/auth: `web/src/lib/settings.svelte.ts`, `web/src/lib/supabase.svelte.ts`
