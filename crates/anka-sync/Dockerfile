# Multi-stage build for anka-sync
FROM rust:1.85-bookworm AS builder
WORKDIR /src

# Workspace manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/anka-core/Cargo.toml crates/anka-core/
COPY crates/anka-sync/Cargo.toml crates/anka-sync/
# Dummy sources so cargo can resolve the workspace member graph
RUN mkdir -p crates/anka-core/src crates/anka-sync/src \
  && echo "pub fn _dummy() {}" > crates/anka-core/src/lib.rs \
  && echo "fn main() {}" > crates/anka-sync/src/main.rs

# Full sources
COPY crates/anka-core crates/anka-core
COPY crates/anka-sync crates/anka-sync

RUN cargo build --release -p anka-sync

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/target/release/anka-sync /usr/local/bin/anka-sync

ENV ANKA_SYNC_LISTEN=0.0.0.0:8788 \
    ANKA_SYNC_DB=/data/sync.db

VOLUME ["/data"]
EXPOSE 8788
USER nobody

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8788 && echo -e "GET /health HTTP/1.0\\r\\n\\r\\n" >&3 && head -n1 <&3 | grep -q 200' || exit 1

ENTRYPOINT ["anka-sync"]
