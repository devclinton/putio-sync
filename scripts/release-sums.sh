#!/usr/bin/env bash
# Writes <dir>/SHA256SUMS over every file in <dir>, then appends the .bin lines that
# scripts/release-patches.sh wrote (contracts/release-assets.md, feature 055).
#   scripts/release-sums.sh <dir> [<bin-sums>]
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <dir> [<bin-sums>]" >&2
    exit 2
fi
dir="$1"
bin_sums="${2:-}"
[ -z "$bin_sums" ] || bin_sums=$(cd "$(dirname "$bin_sums")" && pwd)/$(basename "$bin_sums")

cd "$dir"
rm -f SHA256SUMS SHA256SUMS.minisig
if command -v sha256sum >/dev/null; then
    sha256sum -- * > SHA256SUMS
else
    shasum -a 256 -- * > SHA256SUMS
fi
[ -z "$bin_sums" ] || cat "$bin_sums" >> SHA256SUMS
cat SHA256SUMS
