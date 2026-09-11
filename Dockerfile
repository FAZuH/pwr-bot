FROM rust:1.92-slim-bookworm AS build
LABEL org.opencontainers.image.source="https://github.com/FAZuH/pwr-bot"

# Required by openssl-sys and boring-sys2
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev build-essential cmake libclang-dev git libfontconfig1-dev libpq-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache build dependencies
COPY Cargo.toml Cargo.lock ./
COPY ./crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    mkdir -p crates/pwr-plugin-protocol/src && \
    touch crates/pwr-plugin-protocol/src/lib.rs && \
    cargo build --release && \
    rm -rf src crates

# Build app
COPY ./assets ./assets
COPY ./src ./src
COPY ./crates ./crates
COPY ./migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release

FROM debian:bookworm-slim AS app
RUN apt-get update && apt-get install -y libfontconfig1 libpq5 && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/migrations /app/migrations
COPY --from=build /app/target/release/pwr-bot /app/pwr-bot
COPY --from=build /app/target/release/feed-settings /app/feed-settings
COPY --from=build /app/target/release/settings /app/settings
COPY --from=build /app/target/release/voice-settings /app/voice-settings
COPY --from=build /app/target/release/welcome-settings /app/welcome-settings

WORKDIR /app
CMD ["./pwr-bot"]
