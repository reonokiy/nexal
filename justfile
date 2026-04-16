# Default: build the nexal-agent binary (release).
default: agent

# Build the in-container agent binary in release mode.
agent:
    cargo build --release -p nexal-agent

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

# Run the Bun frontend in dev mode (watch).
nexal:
    bun run dev

# Run Bun frontend once (no watch).
nexal-start:
    bun run start

# Typecheck Bun frontend.
nexal-typecheck:
    bun run typecheck

# Run the TUI client.
tui:
    bun run src/tui.ts
