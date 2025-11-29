# Multi-stage Dockerfile for NEAR Treasury Sandbox Test Environment
# Builds and runs:
# - NEAR Sandbox (via near-sandbox Rust crate)
# - Bulk Payment API (REST service)
# - Sputnik DAO Indexer (proposal caching)

# ============================================================================
# Stage 1: Build the Bulk Payment Contract
# ============================================================================
# cargo-near 0.17.0 requires Rust 1.89+, but the contract needs Rust 1.86
FROM rust:1.89 AS contract-builder

WORKDIR /build

# Install system dependencies for cargo-near (hidapi requires libudev)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-near for building the contract (requires Rust 1.89)
RUN cargo install cargo-near --version 0.17.0

# Install Rust 1.86 toolchain for building the contract
RUN rustup install 1.86.0 && rustup target add wasm32-unknown-unknown --toolchain 1.86.0

# Copy the contract source
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Create rust-toolchain.toml to use Rust 1.86 for this crate
RUN echo '[toolchain]\nchannel = "1.86.0"' > rust-toolchain.toml

# Build the WASM contract (uses Rust 1.86 via rust-toolchain.toml)
RUN cargo near build non-reproducible-wasm

# ============================================================================
# Stage 2: Build the Sandbox Initializer
# ============================================================================
FROM rust:1.89 AS sandbox-builder

WORKDIR /build

# Install system dependencies for near-sandbox
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the sandbox-init source
COPY sandbox/sandbox-init/Cargo.toml ./
COPY sandbox/sandbox-init/src ./src

# Build the sandbox initializer
RUN cargo build --release

# ============================================================================
# Stage 3: Build the Bulk Payment API
# ============================================================================
FROM rust:1.89 AS api-builder

WORKDIR /build

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the API source
COPY bulk-payment-api/Cargo.toml bulk-payment-api/Cargo.lock ./
COPY bulk-payment-api/src ./src

# Build the API
RUN cargo build --release

# ============================================================================
# Stage 4: Build the Sputnik Indexer
# ============================================================================
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS indexer-planner
RUN git clone --depth 1 https://github.com/near-daos/sputnik-dao-caching-api-server.git .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS indexer-builder
# Clone the repository first
RUN git clone --depth 1 https://github.com/near-daos/sputnik-dao-caching-api-server.git .
# Copy the recipe and cook dependencies (this uses the cloned Cargo.toml)
COPY --from=indexer-planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
# Build the project (source files are already present from git clone)
RUN cargo build --release --bin sputnik-indexer

# ============================================================================
# Stage 5: Runtime environment
# ============================================================================
FROM debian:trixie-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    curl \
    supervisor \
    ca-certificates \
    socat \
    && rm -rf /var/lib/apt/lists/*

# Create application directory
WORKDIR /app

# Copy built binaries
COPY --from=sandbox-builder /build/target/release/sandbox-init /usr/local/bin/
COPY --from=api-builder /build/target/release/bulk-payment-api /usr/local/bin/
COPY --from=indexer-builder /app/target/release/sputnik-indexer /usr/local/bin/

# Copy contract WASM (specific filename from cargo-near build)
COPY --from=contract-builder /build/target/near/near_treasury_bulk_payment_contract.wasm /app/contracts/bulk_payment.wasm

# Copy supervisord configuration
COPY sandbox/supervisord.conf /etc/supervisor/conf.d/

# Copy sputnikdao2.wasm if available (for sample DAO)
COPY sandbox/contracts/ /app/contracts/

# Create data directory for persistent storage
RUN mkdir -p /data /var/log

# Expose ports
# 3030 - NEAR Sandbox RPC
# 8080 - Bulk Payment API
# 5001 - Sputnik Indexer
EXPOSE 3030 8080 5001

# Health check
# Note: Sandbox RPC is on port 3030 (proxied from internal 3031)
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run supervisord to manage all services
CMD ["/usr/bin/supervisord", "-c", "/etc/supervisor/conf.d/supervisord.conf"]
