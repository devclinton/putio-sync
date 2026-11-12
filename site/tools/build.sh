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
# One more book per catalog in site/i18n (machine-translated, see
# site/tools/translate-docs.py), each under docs/<lang>/
langs='"en"'
for catalog in "$root"/site/i18n/*.json; do
    [ -e "$catalog" ] || continue
    lang="$(basename "$catalog" .json)"
    MDBOOK_BOOK__LANGUAGE="$lang" \
    MDBOOK_BUILD__BUILD_DIR="$out/docs/$lang" \
    MDBOOK_OUTPUT__HTML__SITE_URL="/putio-sync/docs/$lang/" \
        mdbook build "$root/site/book"
    langs="$langs,\"$lang\""
done
# Read by the language picker (site/book/lang-picker.js)
echo "[$langs]" > "$out/docs/languages.json"
# mdBook's sidebar toggle is a <label> with aria-expanded, which axe flags as
# critical; role="button" makes the attribute valid. Code blocks scroll
# sideways, so they need to take keyboard focus.
find "$out/docs" -name '*.html' -exec sed -i.bak \
    -e 's/<label id="mdbook-sidebar-toggle" /<label id="mdbook-sidebar-toggle" role="button" /' \
    -e 's/<pre><code/<pre><code tabindex="0"/g' {} +
find "$out/docs" -name '*.html.bak' -delete
# The landing page, and one more per catalog in site/i18n/landing under <lang>/
# (machine-translated too, see site/tools/landing_i18n.py)
python3 "$root/site/tools/landing_i18n.py" build "$out" "$version"
for f in style.css install.js; do
    cp "$root/site/$f" "$out/$f"
done
cp -R "$root/site/screenshots" "$out/" 2>/dev/null || true
cp "$root/snap/gui/putio-sync.png" "$out/icon.png"
touch "$out/.nojekyll"
echo "Built putio-sync $version site in $out"
