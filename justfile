# Default: build everything then run dev.
default: dev

# Build the in-container agent binary in release mode.
agent:
    cargo build --release -p nexal-agent

# Build the gateway binary in release mode.
gateway:
    cargo build --release -p nexal-gateway

# Debug build of the agent.
agent-dev:
    cargo build -p nexal-agent

# Run checks on the whole workspace.
check:
    cargo check

# Run all tests.
test:
    cargo test

# Clean build artifacts.
clean:
    cargo clean

# Build Rust binaries then run Bun in dev mode (watch).
dev: agent gateway
    bun run dev

# Run Bun frontend once (no watch), after building Rust binaries.
start: agent gateway
    bun run start

# Typecheck Bun frontend.
typecheck:
    bun run typecheck

# Run the TUI client.
tui:
    bun run src/tui.ts
