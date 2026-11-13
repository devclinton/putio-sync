#!/usr/bin/env bash
# Writes the 055 update assets (contracts/release-assets.md) for one new version:
# putio-sync-<ver>-<target>.xz per new binary, and putio-sync-<from>-to-<ver>-<target>.bsdiff.xz
# for every older binary of the same target. Also writes the SHA256SUMS lines for the
# uncompressed new binaries (putio-sync-<ver>-<target>.bin) to <bin-sums>.
# Both folders hold putio-sync-<ver>-<target>.bin files, as scripts/release-unpack.sh writes.
#   scripts/release-patches.sh <old-dir> <new-dir> <out-dir> <bin-sums>
# MAKE_PATCH is the patch tool; by default it builds the make-patch example.
set -euo pipefail

if [ $# -ne 4 ]; then
    echo "usage: $0 <old-dir> <new-dir> <out-dir> <bin-sums>" >&2
    exit 2
fi
old="$1"
new="$2"
out="$3"
bin_sums="$4"
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"

if [ -z "${MAKE_PATCH:-}" ]; then
    (cd "$here/.." && cargo build --release --locked --quiet -p putio-sync-update --example make-patch)
    MAKE_PATCH="${CARGO_TARGET_DIR:-$here/../target}/release/examples/make-patch"
fi

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

re='^putio-sync-([0-9]+\.[0-9]+\.[0-9]+)-([a-z0-9_-]+)\.bin$'
version=""
: > "$bin_sums"
for bin in "$new"/*.bin; do
    [[ "$(basename "$bin")" =~ $re ]] || continue
    v="${BASH_REMATCH[1]}" target="${BASH_REMATCH[2]}"
    if [ -n "$version" ] && [ "$version" != "$v" ]; then
        echo "$new holds more than one version ($version, $v)" >&2
        exit 1
    fi
    version="$v"
    echo "$(sha256 "$bin")  putio-sync-$v-$target.bin" >> "$bin_sums"
    xz -9e --check=crc64 -c "$bin" > "$out/putio-sync-$v-$target.xz"
    echo "wrote putio-sync-$v-$target.xz"
done
if [ -z "$version" ]; then
    echo "no putio-sync-<ver>-<target>.bin in $new" >&2
    exit 1
fi

# Each older version, and whether it has each new target
froms=$(for f in "$old"/*.bin; do
    [[ "$(basename "$f")" =~ $re ]] && [ "${BASH_REMATCH[1]}" != "$version" ] && echo "${BASH_REMATCH[1]}"
done | sort -u)
for from in $froms; do
    for bin in "$new"/putio-sync-"$version"-*.bin; do
        [[ "$(basename "$bin")" =~ $re ]] || continue
        target="${BASH_REMATCH[2]}"
        prev="$old/putio-sync-$from-$target.bin"
        if [ ! -f "$prev" ]; then
            echo "skip: $from has no $target binary"
            continue
        fi
        name="putio-sync-$from-to-$version-$target.bsdiff.xz"
        tmp=$(mktemp)
        "$MAKE_PATCH" "$prev" "$bin" "$tmp"
        xz -9e --check=crc64 -c "$tmp" > "$out/$name"
        rm -f "$tmp"
        echo "wrote $name ($(wc -c < "$out/$name" | tr -d ' ') bytes)"
    done
done
