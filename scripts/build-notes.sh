#!/usr/bin/env bash
# Markdown notes for one build, for a workflow's step summary: the commits in
# the push that triggered it (from $GITHUB_EVENT_PATH), then the CHANGELOG.md
# section for the given version, or Unreleased.
set -euo pipefail

version="${1:-}"
here="$(dirname "$0")"

if [ -n "${GITHUB_EVENT_PATH:-}" ] && [ -f "$GITHUB_EVENT_PATH" ]; then
    commits=$(jq -r '.commits[]? | "- \(.message | split("\n")[0]) (`\(.id[0:7])`)"' "$GITHUB_EVENT_PATH")
    if [ -n "$commits" ]; then
        echo "### Commits in this build"
        echo ""
        echo "$commits"
        echo ""
    fi
fi

section=$(bash "$here/changelog-section.sh" "${version:-Unreleased}")
if [ -z "$section" ] && [ -n "$version" ]; then
    version=""
    section=$(bash "$here/changelog-section.sh" Unreleased)
fi
echo "### Changelog (${version:-Unreleased})"
echo ""
echo "${section:-No entries.}"
