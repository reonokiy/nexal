# Sandbox image the gateway's Fly backend boots as agent Machines.
# Must ship /usr/local/bin/nexal-agent (the Fly backend cannot copy a
# host binary in, unlike podman). Built+pushed by .github/workflows/
# docker.yml on changes to this file → ghcr.io/reonokiy/nexal-sandbox.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p nexal-agent

FROM python:3.13-slim-trixie
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/nexal-agent /usr/local/bin/nexal-agent
RUN chmod +x /usr/local/bin/nexal-agent
# No ENTRYPOINT: the Fly backend sets the machine's
# init.exec = [/usr/local/bin/nexal-agent, --listen, ws://[::]:9100].
