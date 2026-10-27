#!/usr/bin/env bash
# Every workspace crate must be copied into the Docker build, or the image
# fails to build with a missing workspace member.
set -euo pipefail

cd "$(dirname "$0")/.."
missing=0
for member in $(sed -n '/^members = \[/,/^\]/p' Cargo.toml | grep -o '"[^"]*"' | tr -d '"'); do
    for pattern in "COPY $member/Cargo.toml" "COPY $member ./$member" "$member/src/lib.rs"; do
        if ! grep -qF "$pattern" Dockerfile; then
            echo "Dockerfile is missing: $pattern"
            missing=1
        fi
    done
done
exit "$missing"
