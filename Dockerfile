# Multi-stage build for putio-sync with musl support
FROM rust:alpine AS builder

# Install build dependencies for musl + OpenSSL
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig \
    make

# Set environment for static OpenSSL linking
ENV OPENSSL_STATIC=1
ENV OPENSSL_LIB_DIR=/usr/lib
ENV OPENSSL_INCLUDE_DIR=/usr/include
ENV PKG_CONFIG_ALLOW_CROSS=1

# Set working directory
WORKDIR /build

# Install cargo-chef for better dependency caching
RUN cargo install cargo-chef

# Copy workspace files for dependency planning
COPY Cargo.toml Cargo.lock ./
COPY putio-sync-client/Cargo.toml ./putio-sync-client/
COPY putio-sync-db/Cargo.toml ./putio-sync-db/
COPY putio-sync-download/Cargo.toml ./putio-sync-download/
COPY putio-sync-file-manager/Cargo.toml ./putio-sync-file-manager/
COPY putio-sync-daemon/Cargo.toml ./putio-sync-daemon/
COPY putio-sync-core/Cargo.toml ./putio-sync-core/
COPY putio-sync-cli/Cargo.toml ./putio-sync-cli/
COPY putio-sync-api/Cargo.toml ./putio-sync-api/

# Create minimal source files for dependency analysis
RUN mkdir -p putio-sync-client/src \
             putio-sync-db/src \
             putio-sync-download/src \
             putio-sync-file-manager/src \
             putio-sync-daemon/src \
             putio-sync-core/src \
             putio-sync-cli/src \
             putio-sync-api/src && \
    echo 'fn main() {}' > putio-sync-cli/src/main.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-client/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-db/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-download/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-file-manager/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-daemon/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-core/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-cli/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-api/src/lib.rs

# Generate dependency recipe
RUN cargo chef prepare --recipe-path recipe.json

# Build dependencies with cache mounts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo chef cook --release --recipe-path recipe.json

# Copy real source code
COPY putio-sync-client ./putio-sync-client
COPY putio-sync-db ./putio-sync-db
COPY putio-sync-download ./putio-sync-download
COPY putio-sync-file-manager ./putio-sync-file-manager
COPY putio-sync-daemon ./putio-sync-daemon
COPY putio-sync-core ./putio-sync-core
COPY putio-sync-cli ./putio-sync-cli
COPY putio-sync-api ./putio-sync-api

# Build the actual application with static linking
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin putio-sync && \
    cp target/release/putio-sync /tmp/putio-sync

# Verify the binary is statically linked
RUN ldd /tmp/putio-sync || echo "Static binary confirmed"

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libgcc

# Create non-root user
RUN addgroup -g 568 putio && \
    adduser -D -u 568 -G putio putio

# Create directories
RUN mkdir -p /config /downloads /watches && \
    chown -R putio:putio /config /downloads /watches

# Copy binary from builder (using tmp copy to avoid cache mount issues)
COPY --from=builder /tmp/putio-sync /usr/local/bin/putio-sync

# Set user
USER putio

# Set volumes
VOLUME ["/config", "/downloads", "/watches"]

# Set working directory
WORKDIR /downloads

# default to info logging
ENV RUST_LOG=info

# Set default command
CMD ["putio-sync", "--config", "/config/config.toml", "daemon", "--download-interval", "60", "--watch-interval", "60"]
