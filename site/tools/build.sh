#!/usr/bin/env bash
# Build the project site into target/site (feature 054): the landing page at
# the root and the docs book under docs/. Needs mdbook and python3.
#   site/tools/build.sh            # version from the latest v* tag, else Cargo.toml
#   GITHUB_REF_NAME=v0.2.1 site/tools/build.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/target/site"
version="${GITHUB_REF_NAME:-}"
case "$version" in
    v[0-9]*) version="${version#v}" ;;
    *)
        # The latest release, so a version bump on main doesn't link downloads
        # that don't exist until its tag is pushed
        version="$(git -C "$root" describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
        version="${version#v}"
        [ -n "$version" ] || version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
        ;;
esac

rm -rf "$out"
mkdir -p "$out"
mdbook build "$root/site/book"
# mdBook's sidebar toggle is a <label> with aria-expanded, which axe flags as
# critical; role="button" makes the attribute valid
find "$out/docs" -name '*.html' -exec sed -i.bak 's/<label id="mdbook-sidebar-toggle" /<label id="mdbook-sidebar-toggle" role="button" /' {} +
find "$out/docs" -name '*.html.bak' -delete
for f in index.html style.css install.js; do
    sed "s/{{version}}/$version/g" "$root/site/$f" > "$out/$f"
done
cp -R "$root/site/screenshots" "$out/" 2>/dev/null || true
cp "$root/snap/gui/putio-sync.png" "$out/icon.png"
touch "$out/.nojekyll"
echo "Built putio-sync $version site in $out"
