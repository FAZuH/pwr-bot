FROM rust:1.92-slim-bookworm AS build
LABEL org.opencontainers.image.source="https://github.com/FAZuH/pwr-bot"

# Required by openssl-sys and boring-sys2
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev build-essential cmake libclang-dev git libfontconfig1-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Cache build dependencies
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Build app
COPY ./assets ./assets
COPY ./src ./src
COPY ./migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release

# Includes glibc, libssl.so.3 and libcrypto.so.3, required by app
FROM gcr.io/distroless/cc-debian12:latest AS app

COPY --from=build /app/migrations /app/migrations
COPY --from=build /app/target/release/pwr-bot /app/pwr-bot

WORKDIR /app
CMD ["./pwr-bot"]
