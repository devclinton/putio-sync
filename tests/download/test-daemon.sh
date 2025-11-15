#!/bin/bash
# Daemon mode test (requires manual Ctrl+C)

set -e

BINARY="./target/release/putio-sync"
TEST_DIR="/tmp/putio-daemon-test-$$"

echo "=================================================="
echo "Daemon Mode Test"
echo "=================================================="
echo ""

# Build if needed
if [ ! -f "$BINARY" ]; then
    echo "Building project..."
    cargo build --release
    echo ""
fi

echo "Starting daemon mode with 30 second interval..."
echo "This will run continuously. Press Ctrl+C to stop."
echo "It should shutdown gracefully and show cycle statistics."
echo ""
echo "Starting in 3 seconds..."
sleep 3

$BINARY download --daemon --all --interval 30 --dir "$TEST_DIR" --progress overall

# This will only execute after Ctrl+C
echo ""
echo "Daemon stopped. Cleaning up..."
rm -rf "$TEST_DIR"
echo "✓ Cleanup complete"
echo ""

echo "=================================================="
echo "Daemon test completed!"
echo "=================================================="
