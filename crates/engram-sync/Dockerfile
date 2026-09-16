# Multi-stage build for engram-sync
FROM rust:1.85-bookworm AS builder
WORKDIR /src

# Workspace manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/engram-core/Cargo.toml crates/engram-core/
COPY crates/engram-sync/Cargo.toml crates/engram-sync/
# Dummy sources so cargo can resolve the workspace member graph
RUN mkdir -p crates/engram-core/src crates/engram-sync/src \
  && echo "pub fn _dummy() {}" > crates/engram-core/src/lib.rs \
  && echo "fn main() {}" > crates/engram-sync/src/main.rs

# Full sources
COPY crates/engram-core crates/engram-core
COPY crates/engram-sync crates/engram-sync

RUN cargo build --release -p engram-sync

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/target/release/engram-sync /usr/local/bin/engram-sync

ENV ENGRAM_SYNC_LISTEN=0.0.0.0:8788 \
    ENGRAM_SYNC_DB=/data/sync.db

VOLUME ["/data"]
EXPOSE 8788
USER nobody

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8788 && echo -e "GET /health HTTP/1.0\\r\\n\\r\\n" >&3 && head -n1 <&3 | grep -q 200' || exit 1

ENTRYPOINT ["engram-sync"]
