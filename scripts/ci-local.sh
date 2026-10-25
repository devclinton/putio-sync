#!/usr/bin/env bash
# Local CI gate - the source of truth for merges. GitHub Actions runs this same script.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_step() {
    echo -e "${BLUE}==>${NC} ${GREEN}$1${NC}"
}

print_error() {
    echo -e "${RED}ERROR:${NC} $1"
}

print_success() {
    echo -e "${GREEN}SUCCESS:${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}WARNING:${NC} $1"
}

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    print_error "Cargo is not installed. Please install Rust from https://rustup.rs/"
    exit 1
fi

# Display Rust version
print_step "Rust version information"
if [ -f rust-toolchain.toml ]; then
    echo "Pinned toolchain: $(grep '^channel' rust-toolchain.toml | cut -d'"' -f2)"
fi
rustc --version
cargo --version
echo ""

# Check formatting
print_step "Checking code formatting..."
if cargo fmt --all -- --check; then
    print_success "Code formatting check passed"
else
    print_error "Code formatting check failed. Run 'cargo fmt --all' to fix."
    exit 1
fi
echo ""

# Run clippy
print_step "Running clippy lints..."
if cargo clippy --all-targets --all-features -- -D warnings; then
    print_success "Clippy check passed"
else
    print_error "Clippy check failed. Please fix the warnings above."
    exit 1
fi
echo ""

# Run tests
print_step "Running tests..."
if cargo test --all-features --verbose; then
    print_success "All tests passed"
else
    print_error "Tests failed"
    exit 1
fi
echo ""

# Build release
print_step "Building release binary..."
if cargo build --release --verbose; then
    print_success "Release build successful"
else
    print_error "Release build failed"
    exit 1
fi
echo ""

print_success "All CI checks passed!"
echo ""
echo "Your code is ready to be committed and pushed."
