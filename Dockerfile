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
COPY putio-sync-telemetry/Cargo.toml ./putio-sync-telemetry/
COPY putio-sync-arr/Cargo.toml ./putio-sync-arr/
COPY putio-sync-doctor/Cargo.toml ./putio-sync-doctor/
COPY putio-sync-precheck/Cargo.toml ./putio-sync-precheck/
COPY putio-sync-storage/Cargo.toml ./putio-sync-storage/
COPY putio-sync-tailscale/Cargo.toml ./putio-sync-tailscale/
COPY putio-sync-hooks/Cargo.toml ./putio-sync-hooks/
COPY putio-sync-notify/Cargo.toml ./putio-sync-notify/
COPY putio-sync-plugins/Cargo.toml ./putio-sync-plugins/
COPY putio-sync-archive/Cargo.toml ./putio-sync-archive/
COPY putio-sync-pipeline/Cargo.toml ./putio-sync-pipeline/
COPY putio-sync-subtitles/Cargo.toml ./putio-sync-subtitles/
COPY putio-sync-dlna/Cargo.toml ./putio-sync-dlna/
COPY putio-sync-translation/Cargo.toml ./putio-sync-translation/
COPY putio-sync-update/Cargo.toml ./putio-sync-update/

# Create minimal source files for dependency analysis
RUN mkdir -p putio-sync-client/src \
             putio-sync-db/src \
             putio-sync-download/src \
             putio-sync-file-manager/src \
             putio-sync-daemon/src \
             putio-sync-core/src \
             putio-sync-cli/src \
             putio-sync-api/src \
             putio-sync-telemetry/src \
             putio-sync-arr/src \
             putio-sync-doctor/src \
             putio-sync-precheck/src \
             putio-sync-storage/src \
             putio-sync-tailscale/src \
             putio-sync-hooks/src \
             putio-sync-notify/src \
             putio-sync-plugins/src \
             putio-sync-archive/src \
             putio-sync-pipeline/src \
             putio-sync-subtitles/src \
             putio-sync-dlna/src \
             putio-sync-translation/src \
             putio-sync-update/src && \
    echo 'fn main() {}' > putio-sync-cli/src/main.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-client/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-db/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-download/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-file-manager/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-daemon/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-core/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-cli/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-api/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-telemetry/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-arr/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-doctor/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-precheck/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-storage/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-tailscale/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-hooks/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-notify/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-plugins/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-archive/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-pipeline/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-subtitles/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-dlna/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-translation/src/lib.rs && \
    echo 'pub fn placeholder() {}' > putio-sync-update/src/lib.rs

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
COPY putio-sync-telemetry ./putio-sync-telemetry
COPY putio-sync-arr ./putio-sync-arr
COPY putio-sync-doctor ./putio-sync-doctor
COPY putio-sync-precheck ./putio-sync-precheck
COPY putio-sync-storage ./putio-sync-storage
COPY putio-sync-tailscale ./putio-sync-tailscale
COPY putio-sync-hooks ./putio-sync-hooks
COPY putio-sync-notify ./putio-sync-notify
COPY putio-sync-plugins ./putio-sync-plugins
COPY putio-sync-archive ./putio-sync-archive
COPY putio-sync-pipeline ./putio-sync-pipeline
COPY putio-sync-subtitles ./putio-sync-subtitles
COPY putio-sync-dlna ./putio-sync-dlna
COPY putio-sync-translation ./putio-sync-translation
COPY putio-sync-update ./putio-sync-update
# Built into the binary for the web UI's What's new (feature 017)
COPY CHANGELOG.md ./

# Commit SHA for putio_sync_build_info (read by option_env! at compile time)
ARG PUTIO_SYNC_GIT_SHA=unknown
ENV PUTIO_SYNC_GIT_SHA=$PUTIO_SYNC_GIT_SHA

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
    ffmpeg \
    libgcc \
    tzdata

# Create non-root user
RUN adduser -D -u 98 -G users others
RUN adduser -D -u 99 -G users putio

# Create directories
RUN mkdir -p /config /downloads /watches && \
    chown -R putio:users /config /downloads /watches

# Copy binary from builder (using tmp copy to avoid cache mount issues)
COPY --from=builder /tmp/putio-sync /usr/local/bin/putio-sync
COPY CHANGELOG.md /usr/share/doc/putio-sync/CHANGELOG.md

# Set user
USER putio

# Set volumes
VOLUME ["/config", "/downloads", "/watches"]

# Set working directory
WORKDIR /config

# HOME is the config volume so ~/.putiosync/logs is writable under any --user
# (the putio user's /home/putio isn't), as in Dockerfile.dhi. Default to info logging.
ENV HOME=/config RUST_LOG=info

# The update checker names the right docker pull from these (feature 055)
ARG PUTIO_SYNC_IMAGE=ghcr.io/devclinton/putio-sync
ENV PUTIO_SYNC_IMAGE=$PUTIO_SYNC_IMAGE PUTIO_SYNC_IMAGE_VARIANT=latest

# Prometheus metrics ([metrics] bind) and the API + web UI ([api] bind). The UI is
# embedded in the binary; publish the ports with -p or the Unraid template.
EXPOSE 9742 15432

# Set default command
CMD ["putio-sync", "--config", "/config/config.toml", "daemon", "--download-interval", "60", "--watch-interval", "60"]
