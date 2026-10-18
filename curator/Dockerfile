# Multi-stage build targeting linux/arm64 and linux/amd64

# ==============================================================================
# Build Stage
# ==============================================================================
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS builder

ARG TARGETARCH

# Install CA certificates and cross-compilation toolchain when targeting arm64 on amd64
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && if [ "$TARGETARCH" = "arm64" ]; then \
        apt-get install -y --no-install-recommends \
            gcc-aarch64-linux-gnu \
            libc6-dev-arm64-cross && \
        rustup target add aarch64-unknown-linux-gnu; \
    fi && \
    rm -rf /var/lib/apt/lists/*

# Create dedicated non-root user and runtime directories on build host
RUN groupadd -g 1000 curator && \
    useradd -u 1000 -g curator -s /bin/bash -m curator && \
    mkdir -p /tmp/data && \
    chown -R 1000:1000 /tmp/data

WORKDIR /app

# Copy static reference assets needed at compile time by reference_provider.rs
COPY data/battlemap-reference-* ./data/

# Copy actual curator source code and embedded UI assets
COPY curator/ ./curator/

WORKDIR /app/curator

# Build release binary with embedded assets (native compilation or cross-linking)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    if [ "$TARGETARCH" = "arm64" ]; then \
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
        cargo build --release --target aarch64-unknown-linux-gnu --bin curator && \
        cp target/aarch64-unknown-linux-gnu/release/curator /usr/local/bin/curator; \
    else \
        cargo build --release --bin curator && \
        cp target/release/curator /usr/local/bin/curator; \
    fi

# ==============================================================================
# Runtime Stage (targets $TARGETPLATFORM, e.g. linux/arm64 or linux/amd64)
# Contains zero RUN instructions to eliminate any QEMU emulation requirement
# ==============================================================================
FROM debian:bookworm-slim AS runtime

# Copy CA certificates, user configuration, data directory and compiled binary
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder --chown=1000:1000 /tmp/data /data
COPY --from=builder /usr/local/bin/curator /usr/local/bin/curator

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
