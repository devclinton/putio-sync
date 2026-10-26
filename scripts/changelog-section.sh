#!/usr/bin/env bash
# Prints one section of CHANGELOG.md without its heading: the given version
# (0.1.2 or v0.1.2), or Unreleased by default. Prints nothing if it's missing.
set -euo pipefail

version="${1:-Unreleased}"
version="${version#v}"
changelog="${CHANGELOG:-$(dirname "$0")/../CHANGELOG.md}"

awk -v want="$version" '
    /^## \[/ {
        if (found) exit
        found = index($0, "## [" want "]") == 1
        next
    }
    found
' "$changelog" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
