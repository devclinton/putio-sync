#!/usr/bin/env bash
# Serves a fake GitHub release of putio-sync <ver> on localhost for the 055 quickstart: the
# releases API answer, the update assets of contracts/release-assets.md built from two local
# binaries, and SHA256SUMS signed by the test key in putio-sync-update/tests/data/.
#   scripts/fake-release.sh <ver> <dir>
# Then run a debug daemon with PUTIO_SYNC_UPDATE_API=http://localhost:8099.
#
# OLD_BIN  the binary that will update (default target/debug/putio-sync)
# FROM     its version (default: what `$OLD_BIN --version` prints)
# NEW_BIN  the binary to offer as <ver> (default $OLD_BIN)
# TARGET   the asset target (default: this machine's)
# PORT     default 8099; NO_SERVE=1 writes everything and exits
set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 <ver> <dir>" >&2
    exit 2
fi
ver="${1#v}"
mkdir -p "$2"
dir=$(cd "$2" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
root="$here/.."
port="${PORT:-8099}"
old_bin="${OLD_BIN:-$root/target/debug/putio-sync}"
new_bin="${NEW_BIN:-$old_bin}"
from="${FROM:-$("$old_bin" --version | awk '{print $NF}')}"
from="${from#v}"

if [ -z "${TARGET:-}" ]; then
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) TARGET=linux-x86_64 ;;
        Linux-aarch64 | Linux-arm64) TARGET=linux-aarch64 ;;
        Darwin-arm64) TARGET=macos-arm64 ;;
        MINGW*-x86_64 | MSYS*-x86_64) TARGET=windows-x64 ;;
        *) echo "set TARGET: no release target for $(uname -s) $(uname -m)" >&2; exit 1 ;;
    esac
fi

work="$dir/.work"
assets="$dir/download/v$ver"
rm -rf "$work" "$assets"
mkdir -p "$work/old" "$work/new" "$assets"
cp "$old_bin" "$work/old/putio-sync-$from-$TARGET.bin"
cp "$new_bin" "$work/new/putio-sync-$ver-$TARGET.bin"

"$here/release-patches.sh" "$work/old" "$work/new" "$assets" "$work/bin-sums"
"$here/release-sums.sh" "$assets" "$work/bin-sums" > /dev/null
(cd "$root" && MINISIGN_PASSWORD=putio-sync-test cargo run --quiet -p putio-sync-update \
    --example sign -- putio-sync-update/tests/data/minisign-test.key \
    "$assets/SHA256SUMS" "putio-sync $ver")

body=$("$here/changelog-section.sh" Unreleased)
[ -n "$body" ] || body=$'### Fixed\n\n- A fake release for testing'

# The fields github.rs reads from GET /releases, plus each asset's download address
BODY="$body" python3 - "$ver" "$assets" "http://localhost:$port" > "$dir/releases" <<'PY'
import datetime, json, os, sys
ver, assets, base = sys.argv[1:]
url = f"{base}/download/v{ver}"
print(json.dumps([{
    "tag_name": f"v{ver}",
    "name": f"v{ver}",
    "draft": False,
    "prerelease": False,
    "published_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "html_url": f"{url}/",
    "body": os.environ["BODY"],
    "assets": [
        {"name": n, "size": os.path.getsize(os.path.join(assets, n)), "browser_download_url": f"{url}/{n}"}
        for n in sorted(os.listdir(assets))
    ],
}], indent=2))
PY
rm -rf "$work"

echo "putio-sync $from -> $ver ($TARGET) in $dir:"
ls -l "$assets"
[ -z "${NO_SERVE:-}" ] || exit 0
echo "Serving on http://localhost:$port (PUTIO_SYNC_UPDATE_API=http://localhost:$port)"
cd "$dir"
exec python3 -m http.server "$port" --bind 127.0.0.1
