#!/usr/bin/env bash
# Fails when this branch has feat: or fix: commits that the base branch doesn't
# and none of them touched CHANGELOG.md. A commit with "[skip changelog]" in its
# message is exempt. Base: $CHANGELOG_BASE, else main, else origin/main.
set -euo pipefail

base="${CHANGELOG_BASE:-}"
if [ -z "$base" ]; then
    for candidate in main origin/main; do
        if git rev-parse --verify --quiet "$candidate" > /dev/null; then
            base="$candidate"
            break
        fi
    done
fi
if [ -z "$base" ] || ! git merge-base "$base" HEAD > /dev/null 2>&1; then
    echo "No base branch to compare with; skipping the changelog check"
    exit 0
fi

range="$(git merge-base "$base" HEAD)..HEAD"
needs_entry=$(git log --no-merges --format='%H %s' "$range" \
    | grep -E '^[0-9a-f]+ (feat|fix)(\(.*\))?!?:' || true)
if [ -z "$needs_entry" ]; then
    echo "No feat: or fix: commits since $base"
    exit 0
fi
if ! git diff --quiet "$range" -- CHANGELOG.md; then
    echo "CHANGELOG.md updated since $base"
    exit 0
fi
if git log --no-merges --format=%B "$range" | grep -qF '[skip changelog]'; then
    echo "Changelog check skipped by a [skip changelog] commit"
    exit 0
fi
echo "These commits since $base change behaviour but CHANGELOG.md has no entry:"
echo "$needs_entry" | cut -c1-8,41- | sed 's/^/  /'
echo "Add a line under [Unreleased] in CHANGELOG.md, or put [skip changelog] in a commit message."
exit 1
