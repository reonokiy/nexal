# Nexal Project Structure

> Last updated: 2026-05-18

## Overview

**Nexal** is a multi-channel AI agent runtime. It consists of:

1. **LLM Server** (`src/`) — Bun/TypeScript daemon that hosts Agents, channels, and tools.
2. **Gateway** (`crates/nexal-gateway/`) — Rust WebSocket gateway that routes traffic and manages sandbox containers.
3. **Agent** (`crates/nexal-agent/`) — Rust sandbox executor that runs inside a container and exposes bash/filesystem/process tools via JSON-RPC over WebSocket.
4. **Web UI** (`web/`) — Svelte 5 frontend that connects to the LLM server via WebSocket.

## Architecture

```
Input Channels         Core (LLM Server)            Gateway              Sandbox Agent
├─ Web UI (Svelte)     ├─ Agent Pool                ├─ WebSocket Proxy     ├─ Bash Tool
├─ Telegram            │   └─ pi-agent-core         │   └─ HMAC Auth       ├─ File System
├─ Discord             ├─ Channel Manager           ├─ Session Manager     ├─ Process Manager
├─ HTTP REST           ├─ Tool Registry             ├─ Backend (Fly.io)    └─ JSON-RPC Server
├─ Cron                │   ├─ bash                  └─ Agent Registry
└─ Heartbeat           │   ├─ worker
                       │   └─ report_to_parent
                       └─ Gateway Client
                           └─ WebSocket (HMAC-signed)

Data Flow:
  User Message → Channel → ChannelManager → AgentPool → Agent (pi-agent-core) → LLM
                                                                  │
                                                                  ▼
                                                           Tool Call (bash/worker)
                                                                  │
                                                                  ▼
                                                           GatewayClient ──► Gateway ──► Sandbox Agent
                                                                  WebSocket        JSON-RPC
                                                                  HMAC-signed      over WS
```

- **LLM Server** (`src/`) — Bun/TypeScript daemon. Receives messages from channels, routes them through `AgentPool` (one `pi-agent-core` Agent per chat), proxies tool calls to the Gateway.
- **Gateway** (`crates/nexal-gateway/`) — Rust WebSocket proxy. HMAC-authenticates LLM server connections, spawns sandbox containers (Fly Machines / Podman / K8s), tunnels traffic to Agents.
- **Agent** (`crates/nexal-agent/`) — Rust sandbox executor. Runs inside a container, exposes JSON-RPC methods for bash, file I/O, and process management over a local WebSocket.
- **Web UI** (`web/`) — Svelte 5 frontend. Connects to the LLM server via WebSocket for chat, or directly polls the Gateway for sandbox monitoring.

## Directory Layout

```
nexal/
├── src/                          # TypeScript core (LLM daemon)
│   ├── index.ts                  # Entry: load config, connect gateway, start channels
│   ├── cli.ts                    # CLI entry point
│   ├── agent-pool.ts             # One Agent per (channel, chatId); lifecycle + debounce
│   ├── config.ts                 # TOML + env config loader (NEXAL_* env vars)
│   ├── settings.ts               # Persisted settings (model config, API keys) in Postgres
│   ├── db.ts                     # Postgres connection (Drizzle ORM)
│   ├── schema.ts                 # Drizzle database schema
│   ├── content.ts                # Message content parsing (images, text)
│   ├── log.ts                    # Structured logging
│   ├── channels/                 # Channel abstraction and implementations
│   │   ├── types.ts              # Channel interface, IncomingMessage, OutgoingReply
│   │   ├── manager.ts            # ChannelManager: starts all channels, routes replies
│   │   ├── telegram.ts           # Telegram bot channel (Bot API)
│   │   ├── discord.ts            # Discord bot channel
│   │   ├── http.ts               # HTTP REST channel + /sandboxes endpoint
│   │   ├── ws.ts                 # WebSocket sub-channel (for web UI)
│   │   ├── cron.ts               # Agent-scheduled cron jobs
│   │   ├── heartbeat.ts          # Periodic heartbeat tick
│   │   └── debounce.ts           # SessionDebouncer: batches follow-up messages
│   ├── commands/                 # Built-in slash commands
│   │   ├── builtin.ts            # /help, /model, /providers, /sandboxes, etc.
│   │   └── registry.ts           # Command registry
│   ├── tools/                    # Agent tools (passed to pi-agent-core)
│   │   ├── bash.ts               # Bash execution via gateway sandbox
│   │   ├── worker.ts             # Spawn/cancel persistent sub-agents
│   │   ├── report_to_parent.ts   # Worker → parent chat reporting
│   │   └── send_update.ts        # Send progress updates mid-turn
│   ├── workers/                  # Persistent worker subsystem
│   │   ├── registry.ts           # WorkerRegistry: spawn/route/cancel/list
│   │   ├── store.ts              # Database persistence for workers
│   │   ├── agent.ts              # WorkerAgent: isolated sub-agent runner
│   │   ├── schema.ts             # Worker DB schema
│   │   └── serialize.ts          # Worker state serialization
│   ├── gateway/                  # Gateway WebSocket client
│   │   ├── client.ts             # GatewayClient (HMAC-signed handshake)
│   │   ├── agent_client.ts       # Per-session Agent WebSocket client
│   │   └── protocol.ts           # Gateway wire protocol types
│   ├── prompts/                  # System prompts
│   │   ├── coordinator.md
│   │   └── executor.md
│   ├── scripts/                  # Debug / smoke test scripts
│   │   ├── smoke-gateway.ts
│   │   ├── smoke-worker.ts
│   │   └── smoke-worker-store.ts
│   └── e2e/                      # End-to-end tests
│       └── gateway.e2e.test.ts
│
├── crates/                       # Rust workspace
│   ├── nexal-agent/              # Sandbox executor agent
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── server.rs         # WebSocket server (JSON-RPC)
│   │   │   ├── executor.rs       # Process execution (via pty)
│   │   │   ├── environment.rs    # Sandbox env setup
│   │   │   ├── transport.rs      # Wire protocol (msgpack)
│   │   │   └── proxy.rs          # Proxy forwarding
│   │   ├── tests/                # Integration tests
│   │   └── Cargo.toml
│   ├── nexal-gateway/            # Gateway server
│   │   ├── src/
│   │   │   ├── bin/nexal-gateway.rs  # CLI binary entry
│   │   │   ├── server.rs         # HTTP + WebSocket server (PrefixedStream)
│   │   │   ├── session.rs        # Session management (HMAC auth)
│   │   │   ├── backend.rs        # Backend trait (spawn sandboxes)
│   │   │   ├── backend/fly.rs    # Fly.io Machines API backend
│   │   │   ├── backend/podman.rs # Podman backend
│   │   │   ├── backend/kubernetes.rs # Kubernetes backend
│   │   │   ├── agent_conn.rs     # Agent connection handling
│   │   │   ├── registry.rs       # Agent registry
│   │   │   ├── pool.rs           # Connection pool
│   │   │   ├── proxy.rs          # Proxy forwarding
│   │   │   ├── config.rs         # Gateway config (TOML)
│   │   │   └── protocol.rs       # Gateway protocol types
│   │   └── Cargo.toml
│   └── utils/                    # Shared Rust utilities
│       ├── pty/                  # PTY subprocess management
│       ├── json-transport/       # JSON-RPC transport helpers
│       ├── absolute-path/        # Path utilities
│       ├── cargo-bin/            # Cargo binary helpers
│       └── certs/                # TLS certificate helpers
│
├── web/                          # Svelte 5 Web UI
│   ├── src/
│   │   ├── App.svelte            # Root layout (sidebar + chat view)
│   │   ├── main.ts               # Entry
│   │   ├── lib/
│   │   │   ├── client.svelte.ts      # Chat reactive state wrapper
│   │   │   ├── settings.svelte.ts    # localStorage-backed settings
│   │   │   ├── router.svelte.ts      # Hash router (#/settings)
│   │   │   ├── markdown.ts           # Markdown rendering
│   │   │   ├── utils.ts              # Tailwind class merging
│   │   │   ├── components/
│   │   │   │   ├── sidebar.svelte        # Left navigation
│   │   │   │   ├── composer.svelte       # Chat input
│   │   │   │   ├── message.svelte        # Chat message bubble
│   │   │   │   ├── empty-state.svelte    # Empty chat placeholder
│   │   │   │   ├── settings-page.svelte  # Settings modal/page
│   │   │   │   ├── sandbox-list.svelte   # Sandbox monitoring panel
│   │   │   │   └── settings/             # Settings sub-components
│   │   │   └── views/
│   │   │       └── chat-view.svelte      # Main chat view
│   │   └── app.css
│   ├── package.json
│   ├── vite.config.ts
│   ├── svelte.config.js
│   └── tsconfig.json
│
├── packages/                     # Shared TypeScript packages
│   └── chat-client/              # WebSocket chat client library
│       ├── src/
│       │   ├── client.ts         # NexalChatClient (WebSocket + msgpack)
│       │   ├── protocol.ts       # Wire protocol
│       │   └── index.ts
│       └── package.json
│
├── skills/                       # opencode agent skills (external)
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
│   ├── chatlog/
│   └── toollog/
│
├── deploy/                       # Deployment configs
│   └── server/
│       └── fly.toml              # LLM server Fly.io config
│
├── docker/                       # Docker scripts
│   └── gateway-entrypoint.sh
│
├── drizzle/                      # Database migrations (Drizzle)
│   ├── meta/
│   └── *.sql
│
├── .github/workflows/            # CI/CD
│   ├── deploy-gateway.yml        # Deploy gateway to Fly.io
│   ├── deploy-server.yml         # Deploy LLM server to Fly.io
│   ├── docker.yml                # Build Docker images
│   └── release.yml               # Release workflow
│
├── Cargo.toml                    # Rust workspace manifest
├── package.json                  # Bun/TS root package + workspaces
├── tsconfig.json                 # TypeScript config
├── fly.toml                      # Gateway Fly.io config (app = "nexal")
├── gateway.Dockerfile            # Gateway Docker image (Rust binary)
├── server.Dockerfile             # LLM server Docker image (Bun)
├── sandbox.Dockerfile            # Sandbox image (includes nexal-agent binary)
├── .env                          # Environment variables (not committed)
├── .env.example                  # Env var template
├── rust-toolchain.toml           # Rust toolchain pin
├── rustfmt.toml                  # Rust formatter config
└── README.md
```

## Key Technologies

| Layer | Stack |
|-------|-------|
| LLM Server | Bun ≥ 1.3, TypeScript, `pi-agent-core`, `pi-ai`, Drizzle ORM |
| Gateway | Rust, Tokio, Axum, `tokio-tungstenite` |
| Agent | Rust, Tokio, `tokio-tungstenite`, `portable-pty` |
| Web UI | Svelte 5, Tailwind CSS 4, Vite, `virtua` |
| Database | Postgres (Neon) |
| Deployment | Fly.io, GitHub Actions |

## Data Flow

### Chat Message Flow

1. User sends message via Telegram / Discord / Web UI
2. `Channel` converts it to `IncomingMessage`
3. `ChannelManager` routes to `AgentPool`
4. `AgentPool.SessionDebouncer` batches follow-ups
5. `Agent.prompt()` drives one LLM turn via `pi-agent-core`
6. LLM may call tools (bash, worker, etc.)
7. Tool calls proxy through `GatewayClient` → Gateway → Agent in sandbox
8. Tool results return to LLM
9. Final reply sent back via `Channel.send()`

### WebSocket Protocol

- **Gateway handshake**: HMAC-SHA256 signed `{ access_key, client_name, ts, nonce, signature }`
- **Wire format**: msgpack (not JSON) for binary efficiency
- **Agent protocol**: JSON-RPC over WebSocket (`initialize` → `initialized` → method calls)

## Configuration

Configuration is loaded from three sources (lowest → highest priority):

1. Built-in defaults
2. `~/.nexal/config.toml`
3. Environment variables prefixed with `NEXAL_` (`__` as nesting separator)

Key env vars (see `.env.example`):

| Variable | Purpose |
|----------|---------|
| `NEXAL_GATEWAY_ACCESS_KEY` | Gateway credential ID |
| `NEXAL_GATEWAY_SECRET_KEY` | Gateway credential secret (HMAC signing) |
| `DATABASE_URL` | Postgres connection string |
| `LLM_API_KEY` | LLM provider API key |
| `NEXAL_MODEL_PROVIDER` | Active provider (e.g., `moonshot`) |
| `NEXAL_MODEL` | Active model ID (e.g., `kimi-k2.5`) |
| `TELEGRAM_BOT_TOKEN` | Telegram Bot API token |
| `DISCORD_BOT_TOKEN` | Discord Bot token |

## Development

### Local Development

```bash
# Start Postgres (or use Neon)
# Set DATABASE_URL in .env

# Run LLM server (dev mode with auto-reload)
bun run dev

# Build Rust workspace
cargo build --release

# Start web UI
cd web && bun run dev
```

### Web UI Default Backend

The Web UI defaults to `wss://gateway.nexal.nokiy.net` as the backend URL. Users can override this in Settings.

### Deploying

- **Gateway**: `flyctl deploy` (auto-deploys on push via GitHub Actions)
- **LLM Server**: `flyctl deploy -c deploy/server/fly.toml`
- **Custom domain**: `gateway.nexal.nokiy.net` (CNAME → `nexal.fly.dev`)

## Testing

```bash
# TypeScript type check
bun run typecheck

# Rust tests
cargo test

# Smoke tests
bun run src/scripts/smoke-gateway.ts
bun run src/scripts/smoke-worker.ts
```
