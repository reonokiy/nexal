FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p nexal-agent

FROM scratch
COPY --from=build /src/target/release/nexal-agent /usr/local/bin/nexal-agent
