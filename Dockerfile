# Multi-stage build for rsbox
FROM rust:1.93-bookworm AS builder

LABEL maintainer="rsbox team"
LABEL description="Build stage for rsbox"

WORKDIR /build

# Install build dependencies
RUN apt-get update && \
    apt-get install -y \
    protobuf-compiler \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY . .

# Build release binary with all features
RUN cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel && \
    strip target/release/rsbox

# Runtime stage
FROM debian:bookworm-slim

LABEL maintainer="rsbox team"
LABEL description="rsbox - Rust sing-box compatible proxy platform"

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    iproute2 \
    iptables \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /build/target/release/rsbox /usr/local/bin/rsbox

# Create directories
RUN mkdir -p /etc/rsbox /var/lib/rsbox /var/log/rsbox

# Create non-root user
RUN useradd -r -u 1000 -s /bin/false rsbox && \
    chown -R rsbox:rsbox /var/lib/rsbox /var/log/rsbox

# Environment variables
ENV CONFIG_PATH=/etc/rsbox/config.json \
    RUST_LOG=info \
    RUST_BACKTRACE=0

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD rsbox version || exit 1

# Expose common ports
EXPOSE 7890 9090

# Volume for data persistence
VOLUME ["/etc/rsbox", "/var/lib/rsbox", "/var/log/rsbox"]

# Note: Use 'root' for TUN mode, 'rsbox' for regular mode
# Switch to non-root user (uncomment for non-TUN mode)
# USER rsbox

ENTRYPOINT ["/usr/local/bin/rsbox"]
CMD ["run", "-c", "/etc/rsbox/config.json"]
