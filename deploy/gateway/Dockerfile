# nexal-gateway control image (deployed to Fly). The gateway has no
# database — it serves the frontend handshake and drives the Fly
# Machines API to spawn agent sandboxes. nexal-agent is bundled only to
# satisfy the gateway's required `agent_bin` arg (unused by the Fly
# backend, which runs the agent from the sandbox image instead).

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p nexal-gateway -p nexal-agent

FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/nexal-gateway /usr/local/bin/nexal-gateway
COPY --from=build /src/target/release/nexal-agent   /usr/local/bin/nexal-agent
COPY deploy/gateway/entrypoint.sh /usr/local/bin/gateway-entrypoint.sh
RUN chmod +x /usr/local/bin/gateway-entrypoint.sh /usr/local/bin/nexal-gateway /usr/local/bin/nexal-agent
ENTRYPOINT ["/usr/local/bin/gateway-entrypoint.sh"]
