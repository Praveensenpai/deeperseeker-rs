# ── Stage 1: Build binary ──
FROM rust:bookworm AS builder

WORKDIR /usr/src/deeperseeker

# Pre-copy manifests to cache dependency builds
COPY Cargo.toml Cargo.lock ./

# Create dummy skeleton for dependency layer caching
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs && \
    cargo build --release && \
    rm -rf src

# Copy project source and embedded assets
COPY src ./src
COPY templates ./templates
COPY static ./static
COPY wasm ./wasm
COPY assets ./assets

# Re-touch source and compile production binary
RUN touch src/main.rs src/lib.rs && cargo build --release

# ── Stage 2: Minimal runtime image ──
FROM debian:bookworm-slim

# Install SSL certificates for DeepSeek HTTPS and curl for container health probes
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl tzdata && \
    rm -rf /var/lib/apt/lists/*

# Run as non-root user for container security
RUN groupadd -g 1000 deeperseeker && \
    useradd -u 1000 -g deeperseeker -m -s /bin/bash deeperseeker

WORKDIR /app

# Copy binary and assets from builder
COPY --from=builder /usr/src/deeperseeker/target/release/deeperseeker /usr/local/bin/deeperseeker
COPY --from=builder /usr/src/deeperseeker/templates ./templates
COPY --from=builder /usr/src/deeperseeker/static ./static
COPY --from=builder /usr/src/deeperseeker/wasm ./wasm
COPY --from=builder /usr/src/deeperseeker/assets ./assets

# Configure persistent data directory
RUN mkdir -p /data && chown -R deeperseeker:deeperseeker /app /data

USER deeperseeker:deeperseeker

ENV DEEPSEEKER_HOST=0.0.0.0 \
    DEEPSEEKER_PORT=4000 \
    DEEPSEEKER_DB_PATH=/data/deeperseeker.db \
    DEEPSEEKER_WASM_PATH=/app/wasm/deepseek_pow_solver.wasm \
    RUST_LOG=info

VOLUME ["/data"]

EXPOSE 4000

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://127.0.0.1:4000/health || exit 1

ENTRYPOINT ["deeperseeker"]
CMD ["serve"]
