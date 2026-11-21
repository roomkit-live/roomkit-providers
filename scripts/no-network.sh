#!/usr/bin/env bash
# Runs a command where no network is reachable but the machine's own
# loopback, so that a test, an example or a bench that lost its way to its
# local fake cannot reach a service: it fails there instead.
#
#     scripts/no-network.sh cargo test
#
# Where no route leads off the machine already (inside a namespace this
# script made, or one made by someone else), the command is run as it is.
# Elsewhere the script makes a network namespace of the kernel, by `unshare`
# or by `bwrap`, whichever this machine lets a user make, and looks again
# from inside before it runs the command there.
#
# Where neither makes one, the script runs nothing and fails, saying why.
# ROOMKIT_PROVIDERS_ALLOW_NETWORK=1 alone has it run the command there, with
# the network reachable, which it says on stderr. Nothing else turns it off.
#
# The command reads what was done in ROOMKIT_PROVIDERS_NO_NETWORK: `1` where
# the script saw no route off the machine, `unavailable` where it ran the
# command with the network reachable. The script sets it for every command
# it runs: a value from outside decides nothing and is not passed on.
#
# The command is also given none of the variables a provider reads of its
# environment: the proxy variables, and those that name certificate
# authorities.
#
# CI runs its tests with no network, or not at all. Ubuntu's AppArmor
# refuses a user a namespace by default: each job of the workflows that runs
# tests lifts that for its own machine first, then runs
# scripts/no-network-test.sh (the step "Let the tests make a network
# namespace"), which fails the job where the script still can make none. No
# workflow sets ROOMKIT_PROVIDERS_ALLOW_NETWORK.
set -euo pipefail

# Whether no route leads off the machine, from the network namespace this
# runs in. The kernel sends a packet only by a route that covers its
# destination, so a namespace whose routes are all loopback's sends nothing
# off the machine, whatever interfaces it holds. The two files list the
# routes of every table, for the namespace of the process that reads them.
# Reading them needs no tool and no privilege, and sends nothing.
#
# - IPv4, /proc/net/fib_trie: each `|--` line is the address of a route, and
#   loopback's are all in 127.0.0.0/8.
# - IPv6, /proc/net/ipv6_route: each line ends with the interface of a
#   route. The file is absent where the kernel has no IPv6.
#
# A file that cannot be read counts as a route off the machine.
no_route_off_the_machine() {
    local mark address route
    [[ -r /proc/net/fib_trie ]] || return 1
    while read -r mark address _; do
        if [[ $mark == '|--' && $address != 127.* ]]; then return 1; fi
    done </proc/net/fib_trie || return 1
    [[ -e /proc/net/ipv6_route ]] || return 0
    [[ -r /proc/net/ipv6_route ]] || return 1
    while read -r route; do
        if [[ ${route##* } != lo ]]; then return 1; fi
    done </proc/net/ipv6_route || return 1
}

# The script's own word to itself, when it runs again inside the namespace
# it made with `unshare` or `bwrap`: it makes no other from there.
made_by=
if [[ ${1:-} == --made-by=* ]]; then
    made_by=${1#--made-by=}
    shift
fi
if (($# == 0)); then
    echo "usage: no-network.sh command [argument...]" >&2
    exit 2
fi

# What the environment says of a network decides nothing.
unset ROOMKIT_PROVIDERS_NO_NETWORK

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

if no_route_off_the_machine; then
    export ROOMKIT_PROVIDERS_NO_NETWORK=1
    exec "$@"
fi
if [[ -n $made_by ]]; then
    echo "no-network.sh: the namespace $made_by made still has a route off the" \
        "machine: not running: $*" >&2
    exit 1
fi

# The script runs again inside the namespace, to look from there.
self=${BASH_SOURCE[0]}
if [[ $self != /* ]]; then self=$PWD/$self; fi
# Why each way of making a namespace was left, for the message.
refused=()
if ! command -v unshare >/dev/null 2>&1 || ! command -v ip >/dev/null 2>&1; then
    refused+=("unshare -rn: unshare or ip is not installed")
elif said=$(unshare -rn true 2>&1); then
    exec unshare -rn sh -c 'ip link set lo up && exec "$@"' sh \
        "$BASH" "$self" --made-by=unshare "$@"
else
    refused+=("unshare -rn: $said")
fi
if ! command -v bwrap >/dev/null 2>&1; then
    refused+=("bwrap --unshare-net: bwrap is not installed")
elif said=$(bwrap --unshare-net --dev-bind / / true 2>&1); then
    exec bwrap --unshare-net --dev-bind / / "$BASH" "$self" --made-by=bwrap "$@"
else
    refused+=("bwrap --unshare-net: $said")
fi

if [[ ${ROOMKIT_PROVIDERS_ALLOW_NETWORK:-} == 1 ]]; then
    export ROOMKIT_PROVIDERS_NO_NETWORK=unavailable
    echo "no-network.sh: this machine lets a user make no network-less namespace," \
        "and ROOMKIT_PROVIDERS_ALLOW_NETWORK=1 is set: running with the network" \
        "reachable: $*" >&2
    exec "$@"
fi
{
    echo "no-network.sh: this machine lets a user make no network-less namespace:" \
        "not running: $*"
    printf '    %s\n' "${refused[@]}"
    echo "ROOMKIT_PROVIDERS_ALLOW_NETWORK=1 runs it with the network reachable."
} >&2
exit 1
