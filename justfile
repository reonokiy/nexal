# Default: build everything in release mode
default: build

# Build nexal-exec-server (release) then nexal (release, with embedded exec-server)
build:
    cargo build --release -p nexal-exec-server
    NEXAL_EXEC_SERVER_BIN={{justfile_directory()}}/target/release/nexal-exec-server \
        cargo build --release -p nexal --features embedded-agent

# Development build (debug, no embedding — uses filesystem search fallback)
dev:
    cargo build -p nexal-exec-server
    cargo build -p nexal

# Build only nexal-exec-server in release mode
exec-server:
    cargo build --release -p nexal-exec-server

# Run checks on the whole workspace
check:
    cargo check

# Run all tests
test:
    cargo test

# Run nexal in idle mode (debug, no embedding)
run-idle *ARGS:
    cargo run -p nexal -- idle {{ARGS}}

# Clean build artifacts
clean:
    cargo clean
