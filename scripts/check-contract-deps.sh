#!/usr/bin/env bash
# Keep the contracts usable without an async runtime, an HTTP client, Python
# or RoomKit: every dependency of the contract crate is named here.
set -euo pipefail

allowed=(
    roomkit-providers-llm
    # The JSON forms: serde, and what the derive macros are built with.
    serde
    serde_core
    serde_derive
    proc-macro2
    quote
    syn
    unicode-ident
    # JSON values, and the ordered map that keeps an object's members in
    # the order they were written (serde_json's `preserve_order`).
    serde_json
    itoa
    memchr
    zmij
    indexmap
    equivalent
    hashbrown
    foldhash
    # The error types.
    thiserror
    thiserror-impl
    # The `Stream` a provider's events and text come as: the trait alone,
    # no executor.
    futures-core
    # The random half of an id minted for a call its server gave none: uuid
    # and the system's random source, with what it is reached through on
    # each platform.
    uuid
    getrandom
    cfg-if
    libc
    r-efi
)

dependencies=$(
    "${CARGO:-cargo}" tree --package roomkit-providers-llm --edges normal,build \
        --prefix none --format '{p}' --locked --target all --all-features |
        awk '{print $1}' | LC_ALL=C sort -u
)
unexpected=$(LC_ALL=C comm -23 \
    <(printf '%s\n' "$dependencies") \
    <(printf '%s\n' "${allowed[@]}" | LC_ALL=C sort -u))
if [[ -n "$unexpected" ]]; then
    printf 'Unexpected roomkit-providers-llm dependencies:\n%s\n' "$unexpected" >&2
    exit 1
fi
echo 'roomkit-providers-llm dependencies: contracts only'
