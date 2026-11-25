# Multi-stage build for minimal image size and maximum security
FROM rust:1.80-slim AS builder

WORKDIR /app

# Install OpenSSL & pkg-config dependencies if needed
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Copy real source code and build final binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Minimal runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/zrx-protection-bot /app/zrx-protection-bot

# Copy config and state files
COPY config.json ./config.json
COPY hacked_users.json ./hacked_users.json

ENV RUST_LOG=info

CMD ["./zrx-protection-bot"]
