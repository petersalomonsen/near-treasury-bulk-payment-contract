# Using cargo-chef for optimized Rust builds
# See: https://fly.io/docs/rust/the-basics/cargo-chef/

FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this layer is cached
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin bulk-payment-api

# Runtime stage - use debian:trixie-slim which has GLIBC 2.38+
FROM debian:trixie-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/bulk-payment-api /usr/local/bin/
EXPOSE 8080
CMD ["bulk-payment-api"]
