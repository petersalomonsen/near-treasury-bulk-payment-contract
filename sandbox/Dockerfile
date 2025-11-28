# Multi-stage Dockerfile for NEAR Treasury Sandbox Test Environment
# Builds and runs:
# - NEAR Sandbox (blockchain)
# - Bulk Payment API (REST service)
# - Sputnik DAO Indexer (proposal caching)

# ============================================================================
# Stage 1: Build the Bulk Payment Contract
# ============================================================================
FROM rust:1.86 AS contract-builder

WORKDIR /build

# Install cargo-near for building the contract
RUN cargo install cargo-near --version 0.14.2

# Copy the contract source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build the WASM contract
RUN cargo near build non-reproducible-wasm --no-docker

# ============================================================================
# Stage 2: Build the Bulk Payment API
# ============================================================================
FROM rust:1.86 AS api-builder

WORKDIR /build

# Copy the API source
COPY bulk-payment-api/Cargo.toml bulk-payment-api/Cargo.lock ./
COPY bulk-payment-api/src ./src

# Build the API
RUN cargo build --release

# ============================================================================
# Stage 3: Build the Sputnik Indexer
# ============================================================================
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS indexer-planner
RUN git clone --depth 1 https://github.com/near-daos/sputnik-dao-caching-api-server.git .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS indexer-builder
COPY --from=indexer-planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
RUN git clone --depth 1 https://github.com/near-daos/sputnik-dao-caching-api-server.git .
RUN cargo build --release --bin sputnik-indexer

# ============================================================================
# Stage 4: Runtime environment
# ============================================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    curl \
    jq \
    supervisor \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install NEAR Sandbox binary
RUN curl -L https://github.com/near/sandbox/releases/latest/download/near-sandbox-linux-amd64.tar.gz \
    | tar -xzf - -C /usr/local/bin

# Create application directory
WORKDIR /app

# Copy built binaries
COPY --from=api-builder /build/target/release/bulk-payment-api /usr/local/bin/
COPY --from=indexer-builder /app/target/release/sputnik-indexer /usr/local/bin/

# Copy contract WASM
COPY --from=contract-builder /build/target/near/*.wasm /app/contracts/bulk_payment.wasm

# Copy initialization script and configuration
COPY sandbox/init-sandbox.sh /app/
COPY sandbox/supervisord.conf /etc/supervisor/conf.d/

# Copy sputnikdao2.wasm if available (for sample DAO)
COPY sandbox/contracts/ /app/contracts/

# Make scripts executable
RUN chmod +x /app/init-sandbox.sh

# Create data directory for persistent storage
RUN mkdir -p /data /var/log

# Expose ports
# 3030 - NEAR Sandbox RPC
# 8080 - Bulk Payment API
# 5001 - Sputnik Indexer
EXPOSE 3030 8080 5001

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:3030/status && \
        curl -f http://localhost:8080/health && \
        curl -f http://localhost:5001/health || exit 1

# Run supervisord to manage all services
CMD ["/usr/bin/supervisord", "-c", "/etc/supervisor/conf.d/supervisord.conf"]
