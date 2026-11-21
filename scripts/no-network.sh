#!/usr/bin/env bash
# Runs a command where no network is reachable but the machine's own
# loopback, so that a test, an example or a bench that lost its way to its
# local fake cannot reach a service: it fails there instead.
#
#     scripts/no-network.sh cargo test
#
# It uses a namespace of the kernel, by `unshare` or by `bwrap`, whichever
# this machine lets a user make. Where neither does, it says so and runs the
# command with the network reachable.
set -euo pipefail

if [[ -n "${ROOMKIT_PROVIDERS_NO_NETWORK:-}" ]]; then
    # Already inside one.
    exec "$@"
fi
export ROOMKIT_PROVIDERS_NO_NETWORK=1

if command -v unshare >/dev/null 2>&1 && command -v ip >/dev/null 2>&1 \
    && unshare -rn true 2>/dev/null; then
    exec unshare -rn sh -c 'ip link set lo up && exec "$@"' sh "$@"
fi
if command -v bwrap >/dev/null 2>&1 && bwrap --unshare-net --dev-bind / / true 2>/dev/null; then
    exec bwrap --unshare-net --dev-bind / / "$@"
fi

# Said once: what this command runs in turn is not told again.
export ROOMKIT_PROVIDERS_NO_NETWORK=unavailable
echo "no-network.sh: this machine lets a user make no network-less namespace" \
    "(unshare -rn, bwrap): running with the network reachable: $*" >&2
exec "$@"
