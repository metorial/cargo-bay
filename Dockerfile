FROM rust:1.83-slim AS builder

WORKDIR /build

RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./

RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY src ./src
COPY examples ./examples
COPY tests ./tests

RUN touch src/main.rs && \
    cargo build --release

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && \
    apt-get install -y ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/docker-registry-proxy /app/docker-registry-proxy

RUN mkdir -p /var/cache/docker-registry-proxy && \
    chmod 755 /var/cache/docker-registry-proxy

RUN useradd -r -u 1000 -s /bin/false registry-proxy && \
    chown -R registry-proxy:registry-proxy /app /var/cache/docker-registry-proxy

USER registry-proxy

EXPOSE 5000

ENV CONFIG_PATH=/app/config.toml
ENV RUST_LOG=docker_registry_proxy=info

CMD ["/app/docker-registry-proxy"]
