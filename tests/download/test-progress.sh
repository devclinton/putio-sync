#!/bin/bash
# Progress bar tests

set -e

BINARY="./target/release/putio-sync"
TEST_DIR="/tmp/putio-progress-test-$$"

echo "=================================================="
echo "Progress Bar Tests"
echo "=================================================="
echo ""

# Build if needed
if [ ! -f "$BINARY" ]; then
    echo "Building project..."
    cargo build --release
    echo ""
fi

echo "Test 1: Default progress (per-file)"
echo "Starting download with per-file progress..."
$BINARY download --all --dir "$TEST_DIR/test1" --workers 1
echo "✓ Per-file progress test complete"
echo ""

echo "Test 2: Overall progress only"
echo "Starting download with overall progress..."
$BINARY download --all --dir "$TEST_DIR/test2" --progress overall --workers 1
echo "✓ Overall progress test complete"
echo ""

echo "Test 3: Individual progress only"
echo "Starting download with individual progress..."
$BINARY download --all --dir "$TEST_DIR/test3" --progress individual --workers 1
echo "✓ Individual progress test complete"
echo ""

echo "Test 4: No progress bars"
echo "Starting download with no progress..."
$BINARY download --all --dir "$TEST_DIR/test4" --progress none
echo "✓ No progress test complete"
echo ""

# Cleanup
echo "Cleaning up test directories..."
rm -rf "$TEST_DIR"
echo "✓ Cleanup complete"
echo ""

echo "=================================================="
echo "All progress bar tests passed!"
echo "=================================================="
