# Default: build everything in release mode
default: build

# Build nexal-exec-server (release) then nexal (release, with embedded exec-server, no TUI)
build:
    cargo build --release -p nexal-exec-server
    NEXAL_EXEC_SERVER_BIN={{justfile_directory()}}/target/release/nexal-exec-server \
        cargo build --release -p nexal --no-default-features --features embedded-agent

# Build and run (release, embedded exec-server). Pass args after --.
run *ARGS: build
    ./target/release/nexal {{ARGS}}

# Development build (debug, no TUI, no embedding)
dev:
    cargo build -p nexal-exec-server
    cargo build -p nexal --no-default-features

# Development build and run. Pass args after --.
dev-run *ARGS: dev
    ./target/debug/nexal {{ARGS}}

# Build with TUI support (release)
build-tui:
    cargo build --release -p nexal-exec-server
    NEXAL_EXEC_SERVER_BIN={{justfile_directory()}}/target/release/nexal-exec-server \
        cargo build --release -p nexal --features embedded-agent,tui

# Build only nexal-exec-server in release mode
exec-server:
    cargo build --release -p nexal-exec-server

# Run checks on the whole workspace
check:
    cargo check

# Run all tests
test:
    cargo test

# Clean build artifacts
clean:
    cargo clean
