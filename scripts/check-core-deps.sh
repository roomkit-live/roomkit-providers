#!/usr/bin/env bash
# Keep the contracts usable without an async runtime, an HTTP client, Python
# or RoomKit: every dependency of the core crate is named here.
set -euo pipefail

allowed=(
    roomkit-providers-core
)

dependencies=$(
    "${CARGO:-cargo}" tree --package roomkit-providers-core --edges normal,build \
        --prefix none --format '{p}' --locked --target all --all-features |
        awk '{print $1}' | LC_ALL=C sort -u
)
unexpected=$(LC_ALL=C comm -23 \
    <(printf '%s\n' "$dependencies") \
    <(printf '%s\n' "${allowed[@]}" | LC_ALL=C sort -u))
if [[ -n "$unexpected" ]]; then
    printf 'Unexpected roomkit-providers-core dependencies:\n%s\n' "$unexpected" >&2
    exit 1
fi
echo 'roomkit-providers-core dependencies: contracts only'
