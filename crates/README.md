# Nexal Rust Workspace

This workspace contains the Rust pieces of Nexal: the sandbox gateway, the sandbox agent, and shared utilities used by both.

## Workspace Members

| Crate | Purpose |
|-------|---------|
| `crates/nexal-gateway` | WebSocket gateway that authenticates LLM server clients, manages sandbox backends, and proxies traffic to agents. |
| `crates/nexal-agent` | JSON-RPC server that runs inside a sandbox and exposes bash/process/filesystem operations. |
| `crates/utils/absolute-path` | Absolute path helpers. |
| `crates/utils/cargo-bin` | Cargo binary path helpers for tests and tooling. |
| `crates/utils/certs` | TLS certificate helpers. |
| `crates/utils/json-transport` | JSON-RPC transport helpers. |
| `crates/utils/pty` | PTY-backed subprocess management. |

Channels, model orchestration, worker orchestration, and high-level application runtime live in the Bun/TypeScript server under `src/`, not in Rust crates.

## Gateway

`nexal-gateway` is the network boundary between the LLM server and sandbox agents.

Key files:

- `crates/nexal-gateway/src/bin/nexal-gateway.rs` — binary entry point.
- `crates/nexal-gateway/src/server.rs` — HTTP/WebSocket server.
- `crates/nexal-gateway/src/config.rs` — gateway config loading.
- `crates/nexal-gateway/src/backend/` — Fly.io, Podman, and Kubernetes backends.
- `crates/nexal-gateway/src/proxy/` — proxying between clients and sandbox agents.

## Agent

`nexal-agent` runs inside the sandbox environment and serves JSON-RPC over WebSocket.

Key files:

- `crates/nexal-agent/src/bin/nexal-agent.rs` — binary entry point.
- `crates/nexal-agent/src/server.rs` and `src/server/` — WebSocket server and RPC services.
- `crates/nexal-agent/src/protocol/` — wire protocol types and errors.
- `crates/nexal-agent/src/process/` — process execution backend.
- `crates/nexal-agent/src/fs/` — filesystem operations.
- `crates/nexal-agent/src/proxy.rs` — proxy support.

## Commands

```bash
# Check the full Rust workspace
cargo check

# Run Rust tests
cargo test

# Build all release binaries
cargo build --release

# Build individual binaries
cargo build --release -p nexal-agent
cargo build --release -p nexal-gateway
```

The root `justfile` also exposes `just agent`, `just gateway`, `just check`, and `just test` wrappers.
