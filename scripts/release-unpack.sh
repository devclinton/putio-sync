#!/usr/bin/env bash
# Pulls the bare putio-sync binaries out of a folder of release assets (as a GitHub
# release has them, or a release-binaries.yml run's artifacts) into
# <out>/putio-sync-<ver>-<target>.bin (contracts/release-assets.md, feature 055).
# Reads the 055 .xz assets, the 053 archives (.tar.gz, Windows .zip) and .bin files;
# skips everything else, patches included.
#   scripts/release-unpack.sh <assets-dir> <out-dir>
set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 <assets-dir> <out-dir>" >&2
    exit 2
fi
assets="$1"
out="$2"
mkdir -p "$out"

ver='[0-9]+\.[0-9]+\.[0-9]+'
for f in "$assets"/*; do
    name=$(basename "$f")
    case "$name" in
        *.bsdiff.xz) continue ;;
    esac
    if [[ "$name" =~ ^putio-sync-($ver)-([a-z0-9_-]+)\.(xz|bin)$ ]]; then
        v="${BASH_REMATCH[1]}" target="${BASH_REMATCH[2]}" kind="${BASH_REMATCH[3]}"
        dest="$out/putio-sync-$v-$target.bin"
        if [ "$kind" = xz ]; then xz -dc "$f" > "$dest"; else cp "$f" "$dest"; fi
    elif [[ "$name" =~ ^putio-sync-($ver)-(linux-x86_64|linux-aarch64|macos-arm64)\.tar\.gz$ ]]; then
        v="${BASH_REMATCH[1]}" target="${BASH_REMATCH[2]}"
        tar -xzf "$f" -O "putio-sync-$v-$target/putio-sync" > "$out/putio-sync-$v-$target.bin"
    elif [[ "$name" =~ ^putio-sync-v($ver)-(x86_64|aarch64)-pc-windows-msvc\.zip$ ]]; then
        v="${BASH_REMATCH[1]}"
        target=windows-x64
        [ "${BASH_REMATCH[2]}" = aarch64 ] && target=windows-arm64
        unzip -p "$f" putio-sync.exe > "$out/putio-sync-$v-$target.bin"
    else
        continue
    fi
    echo "unpacked $name -> putio-sync-$v-$target.bin"
done
