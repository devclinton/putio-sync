#!/bin/bash
set -e

# Debian package build script for putio-sync
# Usage: ./build-deb.sh <version> <architecture>
# Example: ./build-deb.sh 0.1.1 amd64

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored messages
info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Check arguments
if [ $# -ne 2 ]; then
    echo "Usage: $0 <version> <architecture>"
    echo "Example: $0 0.1.1 amd64"
    echo ""
    echo "Supported architectures:"
    echo "  - amd64 (x86_64-unknown-linux-gnu)"
    echo "  - arm64 (aarch64-unknown-linux-gnu)"
    echo "  - armhf (armv7-unknown-linux-gnueabihf)"
    exit 1
fi

VERSION="$1"
DEB_ARCH="$2"

# Map Debian architecture to Rust target
case "$DEB_ARCH" in
    amd64)
        RUST_TARGET="x86_64-unknown-linux-gnu"
        ;;
    arm64)
        RUST_TARGET="aarch64-unknown-linux-gnu"
        ;;
    armhf)
        RUST_TARGET="armv7-unknown-linux-gnueabihf"
        ;;
    *)
        error "Unsupported architecture: $DEB_ARCH"
        ;;
esac

info "Building Debian package for putio-sync v${VERSION} (${DEB_ARCH})"

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${PROJECT_ROOT}/build"
PACKAGE_DIR="${BUILD_DIR}/putio-sync_${VERSION}_${DEB_ARCH}"

info "Project root: ${PROJECT_ROOT}"
info "Build directory: ${BUILD_DIR}"

# Clean previous build
if [ -d "${PACKAGE_DIR}" ]; then
    warn "Removing previous build directory"
    rm -rf "${PACKAGE_DIR}"
fi

# Create build directory
info "Creating package directory structure"
mkdir -p "${PACKAGE_DIR}"

# Copy Debian package structure
info "Copying package structure"
cp -r "${SCRIPT_DIR}/DEBIAN" "${PACKAGE_DIR}/"
cp -r "${SCRIPT_DIR}/usr" "${PACKAGE_DIR}/"

# Update control file with version and architecture
info "Updating control file (Version: ${VERSION}, Architecture: ${DEB_ARCH})"
sed -i.bak "s/^Version: .*/Version: ${VERSION}/" "${PACKAGE_DIR}/DEBIAN/control"
sed -i.bak "s/^Architecture: .*/Architecture: ${DEB_ARCH}/" "${PACKAGE_DIR}/DEBIAN/control"
rm "${PACKAGE_DIR}/DEBIAN/control.bak"

# Build the Rust binary
info "Building Rust binary for target: ${RUST_TARGET}"
cd "${PROJECT_ROOT}"

# Check if target is installed
if ! rustup target list --installed | grep -q "${RUST_TARGET}"; then
    warn "Target ${RUST_TARGET} not installed. Installing..."
    rustup target add "${RUST_TARGET}"
fi

# Build
cargo build --release --target="${RUST_TARGET}" --package putio-sync-cli

# Check if binary was created
BINARY_PATH="${PROJECT_ROOT}/target/${RUST_TARGET}/release/putio-sync"
if [ ! -f "${BINARY_PATH}" ]; then
    error "Binary not found at ${BINARY_PATH}"
fi

# Copy binary to package
info "Copying binary to package"
cp "${BINARY_PATH}" "${PACKAGE_DIR}/usr/bin/putio-sync"
chmod 755 "${PACKAGE_DIR}/usr/bin/putio-sync"

# Compress man page
info "Compressing man page"
gzip -9 -n -c "${PACKAGE_DIR}/usr/share/man/man1/putio-sync.1" \
    > "${PACKAGE_DIR}/usr/share/man/man1/putio-sync.1.gz"
rm "${PACKAGE_DIR}/usr/share/man/man1/putio-sync.1"
chmod 644 "${PACKAGE_DIR}/usr/share/man/man1/putio-sync.1.gz"

# Compress changelog
info "Compressing changelog"
gzip -9 -n -c "${PACKAGE_DIR}/usr/share/doc/putio-sync/changelog.Debian" \
    > "${PACKAGE_DIR}/usr/share/doc/putio-sync/changelog.Debian.gz"
rm "${PACKAGE_DIR}/usr/share/doc/putio-sync/changelog.Debian"
chmod 644 "${PACKAGE_DIR}/usr/share/doc/putio-sync/changelog.Debian.gz"

# Copy additional documentation
info "Copying additional documentation"
if [ -f "${PROJECT_ROOT}/README.md" ]; then
    cp "${PROJECT_ROOT}/README.md" "${PACKAGE_DIR}/usr/share/doc/putio-sync/"
    gzip -9 -n "${PACKAGE_DIR}/usr/share/doc/putio-sync/README.md"
fi

# Set proper permissions
info "Setting permissions"
chmod 755 "${PACKAGE_DIR}/DEBIAN/postinst"
chmod 755 "${PACKAGE_DIR}/DEBIAN/prerm"
chmod 644 "${PACKAGE_DIR}/DEBIAN/control"
chmod 644 "${PACKAGE_DIR}/DEBIAN/copyright"

# Calculate installed size (in KB)
INSTALLED_SIZE=$(du -sk "${PACKAGE_DIR}/usr" | cut -f1)
info "Installed size: ${INSTALLED_SIZE} KB"

# Add installed size to control file
echo "Installed-Size: ${INSTALLED_SIZE}" >> "${PACKAGE_DIR}/DEBIAN/control"

# Build the package
DEB_FILE="${BUILD_DIR}/putio-sync_${VERSION}_${DEB_ARCH}.deb"
info "Building package: $(basename ${DEB_FILE})"
dpkg-deb --build --root-owner-group "${PACKAGE_DIR}" "${DEB_FILE}"

# Generate checksum
info "Generating SHA256 checksum"
cd "${BUILD_DIR}"
sha256sum "$(basename ${DEB_FILE})" > "$(basename ${DEB_FILE}).sha256"

# Package info
info "Package created successfully!"
echo ""
echo "Package: ${DEB_FILE}"
echo "Size: $(du -h ${DEB_FILE} | cut -f1)"
echo "SHA256: $(cat ${DEB_FILE}.sha256 | cut -d' ' -f1)"
echo ""

# Verify package
if command -v lintian &> /dev/null; then
    info "Running lintian checks..."
    lintian "${DEB_FILE}" || warn "Lintian found some issues (see above)"
else
    warn "lintian not installed. Skipping package checks."
    info "Install with: sudo apt-get install lintian"
fi

echo ""
info "To install: sudo dpkg -i ${DEB_FILE}"
info "To inspect: dpkg-deb --contents ${DEB_FILE}"
info "To get info: dpkg-deb --info ${DEB_FILE}"
