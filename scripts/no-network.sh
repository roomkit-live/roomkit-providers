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
#
# The command is also given none of the variables a provider reads of its
# environment: the proxy variables, and those that name certificate
# authorities.
set -euo pipefail

if [[ -n "${ROOMKIT_PROVIDERS_NO_NETWORK:-}" ]]; then
    # Already inside one.
    exec "$@"
fi
export ROOMKIT_PROVIDERS_NO_NETWORK=1

# A provider reads the proxies and the certificate authorities of its
# environment when it is built. Those of the machine that runs the command
# are dropped here: a proxy is out of reach where there is no network, and
# a test must not depend on what a developer's shell names. A test that
# reads such a variable sets it for a child of its own.
shopt -s nocasematch
while IFS= read -r name; do
    case "$name" in
        *_proxy | request_method | ssl_cert_file | ssl_cert_dir) unset "$name" ;;
    esac
done < <(compgen -e)
shopt -u nocasematch

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
