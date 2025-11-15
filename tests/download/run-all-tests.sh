#!/bin/bash
# Run all download command tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=================================================="
echo "Running All Download Command Tests"
echo "=================================================="
echo ""

# Check if logged in
if ! ./target/release/putio-sync list >/dev/null 2>&1; then
    echo "Error: Not logged in to Put.io"
    echo "Please run: ./target/release/putio-sync login --interactive"
    exit 1
fi

# Run basic tests
echo "Running basic tests..."
bash "$SCRIPT_DIR/test-basic.sh"
echo ""

# Run progress tests
echo "Running progress bar tests..."
bash "$SCRIPT_DIR/test-progress.sh"
echo ""

echo "=================================================="
echo "Automated Tests Complete!"
echo "=================================================="
echo ""
echo "To test daemon mode, run:"
echo "  bash $SCRIPT_DIR/test-daemon.sh"
echo ""
echo "To test Ctrl+C handling, run any download and press Ctrl+C"
echo ""
echo "For comprehensive testing, see:"
echo "  Tasks/download-command-testing-guide.md"
echo ""
