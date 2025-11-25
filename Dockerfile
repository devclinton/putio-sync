# Multi-stage build for putio-sync
FROM rust:alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig

# Set working directory
WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY putio-sync-core ./putio-sync-core
COPY putio-sync-cli ./putio-sync-cli
COPY putio-sync-api ./putio-sync-api

# Build release binary with static linking
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo build --release --bin putio-sync

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libgcc

# Create non-root user
RUN addgroup -g 1000 putio && \
    adduser -D -u 1000 -G putio putio

# Create directories
RUN mkdir -p /config /downloads /watches && \
    chown -R putio:putio /config /downloads /watches

# Copy binary from builder
COPY --from=builder /build/target/release/putio-sync /usr/local/bin/putio-sync

# Set user
USER putio

# Set volumes
VOLUME ["/config", "/downloads", "/watches"]

# Set working directory
WORKDIR /downloads

# Set default command
CMD ["putio-sync", "--config", "/config/config.toml", "daemon", "--download-interval", "60", "--watch-interval", "60"]
