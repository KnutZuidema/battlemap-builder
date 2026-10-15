# Multi-stage build targeting linux/arm64 and linux/amd64

# ==============================================================================
# Build Stage
# ==============================================================================
FROM rust:1-bookworm AS builder

# Install build dependencies required for compiling dependencies (e.g. openssl-sys)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy static reference assets needed at compile time by reference_provider.rs
COPY data/battlemap-reference-* ./data/

# Copy actual curator source code and embedded UI assets
COPY curator/ ./curator/

WORKDIR /app/curator

# Build release binary with embedded assets
RUN cargo build --release --bin curator

# ==============================================================================
# Runtime Stage
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Install OpenSSL runtime libraries and certificates
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create dedicated non-root user and group (curator:curator, UID/GID 1000:1000)
RUN groupadd -g 1000 curator && \
    useradd -u 1000 -g curator -s /bin/bash -m curator

# Create mount point /data owned by curator:curator
RUN mkdir -p /data && \
    chown -R curator:curator /data

# Copy compiled binary from builder stage
COPY --from=builder /app/curator/target/release/curator /usr/local/bin/curator

# Set runtime environment variables
ENV PORT=3002 \
    DATA_DIR=/data \
    RUST_LOG=info

# Set non-root user
USER curator

# Expose default port
EXPOSE 3002

# Set working directory
WORKDIR /app

# Run curator binary
ENTRYPOINT ["/usr/local/bin/curator"]
