# nexal

Nexal is a multi-channel AI agent runtime. It combines a Bun/TypeScript LLM
server, a Rust sandbox gateway, Rust sandbox agents, and a Svelte web UI.

## Components

| Component | Path | Purpose |
|-----------|------|---------|
| LLM Server | `src/` | Runs `pi-agent-core` agents, channels, commands, workers, and tool dispatch. |
| Gateway | `crates/nexal-gateway/` | WebSocket gateway that authenticates clients and manages sandbox backends. |
| Agent | `crates/nexal-agent/` | JSON-RPC server inside a sandbox for bash, file, and process operations. |
| Web UI | `web/` | Svelte 5 frontend using the shared chat client over WebSocket. |
| Chat client | `packages/chat-client/` | MsgPack WebSocket client/protocol package. |
| Tape types | `packages/tape/` | Shared append-only fact tape interfaces. |

## Entry Points

- `src/cli.ts` is the CLI/script entry used by `bun run start`, `bun run dev`, and the `nexal` bin.
- `src/index.ts` contains server startup: config loading, migrations, DB-backed settings, gateway connection, workers, and channels.
- `crates/nexal-gateway/src/bin/nexal-gateway.rs` starts the gateway.
- `crates/nexal-agent/src/bin/nexal-agent.rs` starts the sandbox agent.
- `web/src/main.ts` and `web/src/App.svelte` start the web UI.

## Runtime Model

Incoming messages arrive through WebSocket, HTTP, Telegram, GitHub, heartbeat,
or cron channels. `ChannelManager` routes them to `AgentPool`, which keeps one
agent session per channel/chat identity. Agent tools proxy sandbox operations
through `GatewayClient` to the Rust gateway, then to a per-sandbox `nexal-agent`
JSON-RPC server.

The gateway supports Fly.io Machines, Podman, and Kubernetes backends. Deployed
Fly apps are split between the gateway (`fly.toml`, app `nexal`) and LLM server
(`deploy/server/fly.toml`, app `nexal-server`).

## Configuration

The Bun server loads structural config from defaults, `~/.nexal/config.toml` or
`NEXAL_CONFIG_PATH`, and `NEXAL_*` environment overrides. Gateway credentials and
database connection values still come from environment/config.

Model, provider, auth, channel, and tool API key settings are DB-backed KV
settings in Postgres. They are managed by the Web UI and slash commands such as
`/model`, `/apikey`, `/providers`, and `/settings` rather than only by `.env`.

The web UI stores browser settings in `localStorage` with the `nexal.` prefix and
defaults to `wss://api.nexal.nokiy.net` unless `VITE_NEXAL_BACKEND` is set.

## Development

```bash
# LLM server with watch mode
bun run dev

# TypeScript type check
bun run typecheck

# TypeScript tests
bun test

# Web UI dev server
cd web && bun run dev

# Web UI checks
cd web && bun run check

# Rust workspace
cargo check
cargo test
cargo build --release
```

The `justfile` also provides common wrappers: `just dev`, `just start`,
`just check`, `just test`, `just agent`, and `just gateway`.

## Database

Drizzle migrations live in `drizzle/`. Schema changes are made in `src/schema.ts`
and related schema modules, then generated with:

```bash
bun run db:generate
```

## Deployment

- Gateway: `deploy/gateway/{Dockerfile,fly.toml,entrypoint.sh}`, `.github/workflows/deploy-gateway.yml`.
- LLM Server: `deploy/server/{Dockerfile,fly.toml}`, `.github/workflows/deploy-server.yml`.
- Web: `.github/workflows/deploy-web-pages.yml` and `.github/workflows/web-build.yml`.
- Sandbox images: `deploy/sandbox/Dockerfile`, `deploy/agent/Dockerfile`, `.github/workflows/docker.yml` (sandbox), `.github/workflows/release.yml` (agent).

See `docs/structure.md` for the detailed project map and data flow.
