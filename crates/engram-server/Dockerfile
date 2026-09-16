# Self-hosted Engram: API + remote MCP + UI + media
FROM rust:1.85-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates/engram-core/Cargo.toml crates/engram-core/
COPY crates/engram-apkg/Cargo.toml crates/engram-apkg/
COPY crates/engram-serve/Cargo.toml crates/engram-serve/
COPY crates/engram-mcp/Cargo.toml crates/engram-mcp/
COPY crates/engram-server/Cargo.toml crates/engram-server/

RUN mkdir -p crates/engram-core/src crates/engram-apkg/src \
  crates/engram-serve/src crates/engram-mcp/src crates/engram-server/src \
  && echo "pub fn _d(){}" > crates/engram-core/src/lib.rs \
  && echo "pub fn _d(){}" > crates/engram-apkg/src/lib.rs \
  && echo "pub fn _d(){}" > crates/engram-serve/src/lib.rs \
  && echo "fn main(){}" > crates/engram-serve/src/main.rs \
  && echo "pub fn _d(){}" > crates/engram-mcp/src/lib.rs \
  && echo "fn main(){}" > crates/engram-mcp/src/main.rs \
  && echo "fn main(){}" > crates/engram-server/src/main.rs

COPY crates/engram-core crates/engram-core
COPY crates/engram-apkg crates/engram-apkg
COPY crates/engram-serve crates/engram-serve
COPY crates/engram-mcp crates/engram-mcp
COPY crates/engram-server crates/engram-server

RUN cargo build --release -p engram-server

# Optional: bake web UI if present in build context
COPY apps/desktop/dist /opt/engram/dist

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/target/release/engram-server /usr/local/bin/engram-server
COPY --from=builder /opt/engram/dist /app/dist

ENV ENGRAM_COLLECTION=/data/collection.egdb \
    ENGRAM_SERVER_LISTEN=0.0.0.0:8787 \
    ENGRAM_SERVER_STATIC=/app/dist

VOLUME ["/data"]
EXPOSE 8787
# run as non-root; ensure /data is writable via volume permissions
USER nobody

HEALTHCHECK --interval=30s --timeout=3s --start-period=8s \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8787 && printf "GET /health HTTP/1.0\r\n\r\n" >&3 && head -n1 <&3 | grep -q 200' || exit 1

ENTRYPOINT ["engram-server"]
