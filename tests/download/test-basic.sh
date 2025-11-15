#!/bin/bash
# Basic download command tests

set -e

BINARY="./target/release/putio-sync"

echo "=================================================="
echo "Basic Download Command Tests"
echo "=================================================="
echo ""

# Build if needed
if [ ! -f "$BINARY" ]; then
    echo "Building project..."
    cargo build --release
    echo ""
fi

# Test 1: Help command
echo "Test 1: Display help"
$BINARY download --help
echo "✓ Help displayed successfully"
echo ""

# Test 2: List files (dry run to see what would be downloaded)
echo "Test 2: List available files"
$BINARY list
echo "✓ File list retrieved successfully"
echo ""

# Test 3: Download with progress=none (for non-interactive testing)
echo "Test 3: Download all files with no progress bars"
$BINARY download --all --progress none --dir /tmp/putio-test-$$
echo "✓ Download completed"
echo ""

# Test 4: Skip existing
echo "Test 4: Skip existing files"
$BINARY download --all --skip-existing --progress none --dir /tmp/putio-test-$$
echo "✓ Skip existing worked"
echo ""

# Cleanup
echo "Cleaning up test directory..."
rm -rf /tmp/putio-test-$$
echo "✓ Cleanup complete"
echo ""

echo "=================================================="
echo "All basic tests passed!"
echo "=================================================="
