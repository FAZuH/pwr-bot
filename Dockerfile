FROM rust:1.88-slim-bookworm AS build
LABEL org.opencontainers.image.source "https://github.com/FAZuH/pwr-bot"

# Required by openssl-sys
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock /app/
COPY ./src /app/src
COPY ./migrations /app/migrations

WORKDIR /app
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --release

FROM debian:bookworm-slim AS app

# Install required libssl.so.3
RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=build /app/migrations /app/migrations
COPY --from=build /app/target/release/pwr-bot /app/pwr-bot

WORKDIR /app
CMD ["./pwr-bot"]
