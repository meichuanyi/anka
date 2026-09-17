# Self-hosted Anka: API + remote MCP + UI + media
FROM rust:1.85-bookworm AS builder
WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates/anka-core/Cargo.toml crates/anka-core/
COPY crates/anka-apkg/Cargo.toml crates/anka-apkg/
COPY crates/anka-serve/Cargo.toml crates/anka-serve/
COPY crates/anka-mcp/Cargo.toml crates/anka-mcp/
COPY crates/anka-server/Cargo.toml crates/anka-server/

RUN mkdir -p crates/anka-core/src crates/anka-apkg/src \
  crates/anka-serve/src crates/anka-mcp/src crates/anka-server/src \
  && echo "pub fn _d(){}" > crates/anka-core/src/lib.rs \
  && echo "pub fn _d(){}" > crates/anka-apkg/src/lib.rs \
  && echo "pub fn _d(){}" > crates/anka-serve/src/lib.rs \
  && echo "fn main(){}" > crates/anka-serve/src/main.rs \
  && echo "pub fn _d(){}" > crates/anka-mcp/src/lib.rs \
  && echo "fn main(){}" > crates/anka-mcp/src/main.rs \
  && echo "fn main(){}" > crates/anka-server/src/main.rs

COPY crates/anka-core crates/anka-core
COPY crates/anka-apkg crates/anka-apkg
COPY crates/anka-serve crates/anka-serve
COPY crates/anka-mcp crates/anka-mcp
COPY crates/anka-server crates/anka-server

RUN cargo build --release -p anka-server

# Optional: bake web UI if present in build context
COPY apps/desktop/dist /opt/anka/dist

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/target/release/anka-server /usr/local/bin/anka-server
COPY --from=builder /opt/anka/dist /app/dist

ENV ANKA_COLLECTION=/data/collection.akdb \
    ANKA_SERVER_LISTEN=0.0.0.0:8787 \
    ANKA_SERVER_STATIC=/app/dist

VOLUME ["/data"]
EXPOSE 8787
# run as non-root; ensure /data is writable via volume permissions
USER nobody

HEALTHCHECK --interval=30s --timeout=3s --start-period=8s \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8787 && printf "GET /health HTTP/1.0\r\n\r\n" >&3 && head -n1 <&3 | grep -q 200' || exit 1

ENTRYPOINT ["anka-server"]
