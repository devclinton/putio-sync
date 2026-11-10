#!/bin/bash
set -e

# Generate cargo-sources.json for Flatpak offline builds
# This script creates a JSON file with all Cargo dependencies
# Required for Flatpak builds on Flathub

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUTPUT_FILE="${PROJECT_ROOT}/cargo-sources.json"

echo "Generating cargo-sources.json for Flatpak offline builds..."
echo "Project root: ${PROJECT_ROOT}"

# Check if Cargo.lock exists
if [ ! -f "${PROJECT_ROOT}/Cargo.lock" ]; then
    echo "Error: Cargo.lock not found. Run 'cargo build' first."
    exit 1
fi

# Check if flatpak-cargo-generator.py is available
GENERATOR_SCRIPT=""

# Try common locations
if [ -f "/usr/share/flatpak-builder/tools/cargo/flatpak-cargo-generator.py" ]; then
    GENERATOR_SCRIPT="/usr/share/flatpak-builder/tools/cargo/flatpak-cargo-generator.py"
elif [ -f "/usr/lib/flatpak-builder/tools/cargo/flatpak-cargo-generator.py" ]; then
    GENERATOR_SCRIPT="/usr/lib/flatpak-builder/tools/cargo/flatpak-cargo-generator.py"
elif command -v flatpak-cargo-generator.py &> /dev/null; then
    GENERATOR_SCRIPT="flatpak-cargo-generator.py"
fi

if [ -z "$GENERATOR_SCRIPT" ]; then
    echo "Error: flatpak-cargo-generator.py not found."
    echo ""
    echo "Install it with one of:"
    echo "  - Fedora: sudo dnf install flatpak-builder"
    echo "  - Ubuntu/Debian: sudo apt-get install flatpak-builder"
    echo "  - Or download from: https://github.com/flatpak/flatpak-builder-tools"
    echo ""
    echo "Alternatively, download the script:"
    echo "  curl -o /tmp/flatpak-cargo-generator.py \\"
    echo "    https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py"
    echo "  python3 /tmp/flatpak-cargo-generator.py ./Cargo.lock -o cargo-sources.json"
    exit 1
fi

echo "Using generator: ${GENERATOR_SCRIPT}"

# Check Python dependencies
if ! python3 -c "import aiohttp, toml" 2>/dev/null; then
    echo "Installing required Python packages..."
    pip3 install --user aiohttp toml
fi

# Generate cargo sources
echo "Generating cargo sources from Cargo.lock..."
python3 "${GENERATOR_SCRIPT}" "${PROJECT_ROOT}/Cargo.lock" -o "${OUTPUT_FILE}"

# Verify the output
if [ -f "${OUTPUT_FILE}" ]; then
    SIZE=$(du -h "${OUTPUT_FILE}" | cut -f1)
    ENTRIES=$(grep -c '"type": "file"' "${OUTPUT_FILE}" || echo "0")
    echo ""
    echo "✓ Successfully generated cargo-sources.json"
    echo "  File: ${OUTPUT_FILE}"
    echo "  Size: ${SIZE}"
    echo "  Entries: ${ENTRIES} dependencies"
    echo ""
    echo "Next steps:"
    echo "1. Uncomment the cargo-sources.json line in io.github.devclinton.putio-sync.yaml"
    echo "2. Test the build: flatpak-builder --force-clean build-dir io.github.devclinton.putio-sync.yaml"
else
    echo "Error: Failed to generate cargo-sources.json"
    exit 1
fi
