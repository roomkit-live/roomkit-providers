#!/usr/bin/env bash
# Checks scripts/no-network.sh: that the command it runs has no route off
# the machine whatever the environment names, that it runs nothing where it
# can make no namespace unless it is told to, and what it tells its command.
#
#     bash scripts/no-network-test.sh
#
# It needs no network and no privilege: bash, `timeout` and, for a listener,
# `perl`. What it can show depends on where it is run, and it says which:
#
# - outside any namespace, everything: the script has a namespace to make,
#   and a `PATH` without `unshare` and `bwrap` stands for a machine that
#   lets a user make none;
# - inside a namespace with no network already (run through the script, or
#   in a sandbox), the script rightly runs its command as it is: that is
#   checked, and what the script does where it can make no namespace is
#   named as not run.
#
# What shows that a command has no network is not the script's own reading
# of the routes: the command asks the kernel for a route to an address off
# the machine, and must reach a listener it starts on loopback.
set -euo pipefail

# This file and the script beside it, found with no tool: the command the
# script is given is this file, run where `PATH` may hold none.
this=${BASH_SOURCE[0]}
if [[ $this != /* ]]; then this=$PWD/$this; fi
script=${this%/*}/no-network.sh

# What a provider reads of its environment, in several cases: every command
# below is run with all of them set, and must be given none.
READ=(HTTPS_PROXY http_proxy ALL_PROXY No_Proxy REQUEST_METHOD SSL_CERT_FILE ssl_cert_dir)

# What the kernel answers, from where this runs, for an address off the
# machine: 192.0.2.1 (TEST-NET-1, which no network routes). It prints
# `unreachable` where no route leads there.
#
# Connecting a UDP socket sends nothing: it only asks the kernel for a
# route, and fails with "network is unreachable" where there is none. Only
# then is a TCP connection tried, as a provider would open one: it cannot
# leave, and must be refused the same way at once. The TCP answer alone
# tells nothing: where a router on the way answers that it has no route, a
# machine with a network is told "network is unreachable" too.
off_the_machine() {
    local socket said status
    for socket in udp tcp; do
        status=0
        said=$(LC_ALL=C timeout 1 bash -c 'exec 3<>"/dev/$1/192.0.2.1/9"' bash "$socket" 2>&1) \
            || status=$?
        if ((status == 0)); then
            echo "$socket: a route leads there"
            return
        elif ((status == 124)); then
            echo "$socket: no answer in a second"
            return
        elif [[ $said != *[Nn]etwork*unreachable* ]]; then
            echo "$socket: $said"
            return
        fi
    done
    echo unreachable
}

# Whether a listener started here, on a port loopback gives it, is reached.
# The listener is Perl's: bash opens a connection and waits for none.
loopback_answers() {
    local from port listener status=1
    exec {from}< <(perl -MIO::Socket::INET -e '
        alarm 5;
        my $listener = IO::Socket::INET->new(Listen => 1, LocalAddr => "127.0.0.1")
            or die "no listener on loopback: $!\n";
        $| = 1;
        print $listener->sockport, "\n";
        $listener->accept;')
    listener=$!
    if read -r port <&"$from" \
        && timeout 5 bash -c 'exec 3<>"/dev/tcp/127.0.0.1/$1"' bash "$port"; then
        status=0
    fi
    exec {from}<&-
    kill "$listener" 2>/dev/null || true
    return "$status"
}

# The command the script is given: this file again, run with `probe`. It
# says what is wrong with what the script gave it, and fails then.
#
# - `probe no-network`: the script said there is no network, which is tried.
# - `probe network FILE`: the script was allowed to run it with the network
#   reachable, and must not say otherwise. FILE is written, as the trace
#   that the command ran.
probe() {
    local expected=$1 ran=${2:-} name answer wrong=0
    local told=${ROOMKIT_PROVIDERS_NO_NETWORK-}
    if [[ -n $ran ]]; then : >"$ran"; fi
    for name in "${READ[@]}"; do
        if [[ -n ${!name+set} ]]; then
            echo "$name is still set"
            wrong=1
        fi
    done
    if [[ $expected == network ]]; then
        if [[ $told == 1 || -z $told ]]; then
            echo "ROOMKIT_PROVIDERS_NO_NETWORK is '$told' where the network is reachable:" \
                "the script says neither 1 nor nothing there"
            wrong=1
        fi
        return "$wrong"
    fi
    if [[ $told != 1 ]]; then
        echo "ROOMKIT_PROVIDERS_NO_NETWORK is '$told', not 1"
        wrong=1
    fi
    answer=$(off_the_machine)
    if [[ $answer != unreachable ]]; then
        echo "192.0.2.1 is not unreachable: $answer"
        wrong=1
    fi
    if ! loopback_answers; then
        echo "a listener on loopback is not reached"
        wrong=1
    fi
    return "$wrong"
}

if [[ ${1:-} == probe ]]; then
    shift
    probe "$@"
    exit
fi

# Runs a command as a developer's shell would: with the variables the
# script must drop, a value of ROOMKIT_PROVIDERS_NO_NETWORK from outside
# (`unset` for none), and no ROOMKIT_PROVIDERS_ALLOW_NETWORK but the one a
# case names.
given() {
    local inherited=$1
    shift
    local -a told=(-u ROOMKIT_PROVIDERS_NO_NETWORK)
    if [[ $inherited != unset ]]; then told=("ROOMKIT_PROVIDERS_NO_NETWORK=$inherited"); fi
    env -u ROOMKIT_PROVIDERS_ALLOW_NETWORK "${told[@]}" "${READ[@]/%/=from-outside}" "$@"
}

# The command has no network: in a namespace the script makes, or as it is
# where the words after the inherited value put the script inside one.
no_network() {
    local inherited=$1
    shift
    given "$inherited" "$@" "$script" "$BASH" "$this" probe no-network
}

# The script fails and runs nothing, with a `PATH` that holds no tool to
# make a namespace with. The words after the inherited value are given to
# the script as its environment.
refuses() {
    local inherited=$1 status=0 said wrong=0
    shift
    rm -f "$ran"
    said=$(given "$inherited" env PATH="$none" "$@" \
        "$script" "$BASH" "$this" probe network "$ran" 2>&1) || status=$?
    if ((status == 0)); then
        echo "the script exits 0"
        wrong=1
    fi
    if [[ -e $ran ]]; then
        echo "the command ran"
        wrong=1
    fi
    if [[ $said != *ROOMKIT_PROVIDERS_ALLOW_NETWORK* ]]; then
        echo "the script does not name ROOMKIT_PROVIDERS_ALLOW_NETWORK: $said"
        wrong=1
    fi
    return "$wrong"
}

# With the same `PATH` and ROOMKIT_PROVIDERS_ALLOW_NETWORK=1, the script
# runs the command, says on stderr that the network is reachable, and does
# not pass on the `1` it was given from outside.
allowed() {
    local status=0 said wrong=0
    rm -f "$ran"
    said=$(given 1 env PATH="$none" ROOMKIT_PROVIDERS_ALLOW_NETWORK=1 \
        "$script" "$BASH" "$this" probe network "$ran" 2>"$work/stderr") || status=$?
    if ((status != 0)); then
        echo "the script exits $status"
        wrong=1
    fi
    if [[ -n $said ]]; then
        echo "$said"
        wrong=1
    fi
    if [[ ! -e $ran ]]; then
        echo "the command did not run"
        wrong=1
    fi
    if [[ $(<"$work/stderr") != *"network reachable"* ]]; then
        echo "stderr does not say that the network is reachable: $(<"$work/stderr")"
        wrong=1
    fi
    return "$wrong"
}

# With a `bwrap` that says it made a namespace and made none, the script
# sees a route off the machine from inside, fails and runs nothing.
not_believed() {
    local status=0 said wrong=0
    rm -f "$ran"
    said=$(given unset env PATH="$liar" \
        "$script" "$BASH" "$this" probe network "$ran" 2>&1) || status=$?
    if ((status == 0)); then
        echo "the script exits 0"
        wrong=1
    fi
    if [[ -e $ran ]]; then
        echo "the command ran"
        wrong=1
    fi
    if [[ $said != *"route off the machine"* ]]; then
        echo "the script does not say that a route leads off the machine: $said"
        wrong=1
    fi
    return "$wrong"
}

# How a title names what the environment says from outside.
named() {
    if [[ $1 == unset ]]; then
        echo "no ROOMKIT_PROVIDERS_NO_NETWORK"
    else
        echo "ROOMKIT_PROVIDERS_NO_NETWORK=$1"
    fi
}

passed=0
failed=0

# Runs one case and tells its outcome, with what the case said when it
# failed.
check() {
    local title=$1 said
    shift
    if said=$("$@" 2>&1); then
        echo "ok: $title"
        passed=$((passed + 1))
    else
        echo "FAILED: $title"
        if [[ -n $said ]]; then echo "    ${said//$'\n'/$'\n'    }"; fi
        failed=$((failed + 1))
    fi
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
ran=$work/ran
# A `PATH` that holds bash and no tool to make a namespace with.
none=$work/none
mkdir "$none"
ln -s "$BASH" "$none/bash"
# A `PATH` whose `bwrap` drops its four words about the namespace and runs
# the command where it is.
liar=$work/liar
mkdir "$liar"
ln -s "$BASH" "$liar/bash"
ln -s "$(type -P true)" "$liar/true"
cat >"$liar/bwrap" <<STAND_IN
#!$BASH
shift 4
exec "\$@"
STAND_IN
chmod +x "$liar/bwrap"

# The ends of the titles below.
no_network="the command has no network"
as_it_is="the command runs as it is, with no network"
nothing="the script fails and runs nothing"
told_so="the command runs, and is told that the network is reachable"

if [[ $(off_the_machine) == unreachable ]]; then
    echo "no-network-test: run inside a namespace with no network"
    for inherited in unset 0 1 anything; do
        check "$(named "$inherited") from outside, inside a namespace already: $as_it_is" \
            no_network "$inherited"
    done
    echo "not run: what the script does where it can make no namespace. Inside one" \
        "already it rightly runs its command, so that cannot be shown from here:" \
        "run this test from outside any namespace."
else
    # How a user makes a namespace here, as someone else than the script.
    maker=none
    elsewhere=()
    if command -v unshare >/dev/null 2>&1 && command -v ip >/dev/null 2>&1 \
        && unshare -rn true 2>/dev/null; then
        maker=unshare
        elsewhere=(unshare -rn sh -c 'ip link set lo up && exec "$@"' sh)
    elif command -v bwrap >/dev/null 2>&1 \
        && bwrap --unshare-net --dev-bind / / true 2>/dev/null; then
        maker=bwrap
        elsewhere=(bwrap --unshare-net --dev-bind / /)
    fi
    echo "no-network-test: run outside any namespace; a user makes one here with: $maker"

    if [[ $maker == none && ${ROOMKIT_PROVIDERS_ALLOW_NETWORK:-} == 1 ]]; then
        echo "not run: that a command has no network. This machine lets a user make" \
            "no namespace, and ROOMKIT_PROVIDERS_ALLOW_NETWORK=1 is set."
    else
        for inherited in 0 1 anything; do
            check "$(named "$inherited") from outside: the script makes a namespace, $no_network" \
                no_network "$inherited"
        done
    fi
    if [[ $maker != none ]]; then
        for inherited in unset 0 1 anything; do
            title="$(named "$inherited") from outside, inside a namespace someone else made"
            check "$title: $as_it_is" no_network "$inherited" "${elsewhere[@]}"
        done
    fi
    check "no namespace can be made: $nothing" \
        refuses unset
    check "no namespace can be made, ROOMKIT_PROVIDERS_NO_NETWORK=1 from outside: $nothing" \
        refuses 1
    check "no namespace can be made, ROOMKIT_PROVIDERS_ALLOW_NETWORK=0: $nothing" \
        refuses unset ROOMKIT_PROVIDERS_ALLOW_NETWORK=0
    check "no namespace can be made, ROOMKIT_PROVIDERS_ALLOW_NETWORK=1: $told_so" \
        allowed
    check "a tool says it made a namespace and made none: $nothing" \
        not_believed
fi

echo "no-network-test: $passed passed, $failed failed"
((failed == 0))
