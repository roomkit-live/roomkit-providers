"""The replay of a provider's recorded wire cases through the Python
package: what a case's provider sent and gave, and where that differs from
what Python did.

A case is asked as RoomKit asks it: its configuration is the package's, its
request an ``AIContext``, and what the provider gives are RoomKit's models,
written back in the JSON form the fixture holds; a failure is RoomKit's own
exception.

The comparison is the Rust replay's (``crates/roomkit-providers-client``,
``src/harness``), in its words, so that the cases a provider answers
otherwise than Python are held to the same statement: the data file its
crate writes of them, ``named_cases.json``.

Nothing here is one provider's. A provider's test module states what the
replay needs of it (``Replays``), asserts its own counts, and makes wrong
the cases of its own fixture.
"""

from __future__ import annotations

import copy
import json
import tomllib
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from urllib.parse import parse_qsl, urlsplit

from fake_endpoint import Fake, Reads, Refused, Reply, Seen, Silent, Whole
from pydantic import BaseModel
from roomkit.providers.ai import (
    API_KEY_METADATA_KEY,
    AIContext,
    AIProvider,
    ProviderError,
    ResponseSchemaError,
)
from roomkit_providers import _native

ROOT = Path(__file__).resolve().parents[2]

# The clock of every fixture, in seconds since 1970: 2026-01-01T00:00:00Z.
# A case whose answer asks a wait until a date replays as Python at no other.
CLOCK = 1_767_225_600.0

# The headers a client writes by itself for the transport, beside those the
# provider decides: any other header the fixture does not record is one the
# provider must not send.
TRANSPORT_HEADERS = ("host", "content-length")

# What a provider names its client with, in the one header compared
# otherwise: the fixture leaves out the one Python's SDK writes, so no case
# records it but one whose configuration sets it. On every other request the
# provider must name its client, with the version of the crates.
_CRATES = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
USER_AGENT = f"roomkit-providers/{_CRATES['workspace']['package']['version']}"

# How a recorded header names the key it carried: the configuration's, or
# the one its request carries for itself.
CONFIG_KEY, REQUEST_KEY = "<config key>", "<request key>"

# What a case records its provider answers of itself: each member whose name
# starts so, which is the name of the provider's property.
SUPPORTS = "supports_"

# What the package says of a request the contract does not read, before the
# reader's own words.
UNREAD = "the request is refused: "

# How a recorded stream of reads ends, as the fake serves it.
ENDINGS = {"the stream ends": "ends", "the connection is lost": "lost"}

# How a case whose fake goes silent is set apart (`set_apart`).
NEVER_ANSWERED, SILENT_UNDER_WAY = "never answered", "silent under way"

# The timeout, in seconds, a case set apart is given in place of its
# configuration's, which ends each silence of its fake.
#
# - Where the fake never answers, nothing but the timeout ends the case,
#   however long anything takes.
# - Where it goes silent under way, the timeout is also the longest the
#   provider waits for each read the fake does send: one that passed first
#   would end the case otherwise than recorded. So it is many times what a
#   read takes to come on a busy machine (one came 100 ms late on a
#   processor shared with thirty-two busy processes, none 250 ms late), and
#   each silence of these cases costs the test that long.
SHORT_TIMEOUTS = {NEVER_ANSWERED: 0.05, SILENT_UNDER_WAY: 1.0}


@dataclass(frozen=True)
class Replays:
    """A provider the replay drives, as its own test module states it."""

    # The name the native module holds the provider under, which the name of
    # its crate ends with.
    vendor: str
    # The provider's class in the package, and its configuration's.
    provider: type[AIProvider]
    config: type[BaseModel]
    # The fixture of the recorded cases, among the fixtures of the crate.
    cases: str
    # The header that carries the provider's key, in lower case: what a
    # provider built without a key does not send.
    key_header: str
    # The crate that holds the fixture, when it is not the vendor's own: a
    # vendor of OpenAI's wire is a module of the OpenAI crate.
    crate: str | None = None
    # The cases the Rust replay names, among the crate's fixtures; ``None``
    # where it names none.
    named_cases: str | None = "named_cases.json"
    # What a case's configuration becomes before the provider is built from
    # it, where the fixture writes a member in a form of its own (a key file
    # built from the fixtures' test key); ``None`` takes it as it is.
    prepared: Callable[[dict[str, Any]], dict[str, Any]] | None = None

    def _read(self, name: str) -> Any:
        crate = self.crate or self.vendor
        fixtures = ROOT / f"crates/roomkit-providers-{crate}/tests/fixtures"
        return json.loads((fixtures / name).read_text(encoding="utf-8"))

    def fixture(self) -> dict[str, Any]:
        """The recorded cases, as the script that captured them wrote them."""
        return self._read(self.cases)

    def named(self) -> dict[str, dict[str, Any]]:
        """The cases the provider answers otherwise than Python, by name, as
        the Rust replay states them."""
        if self.named_cases is None:
            return {}
        return {case["name"]: case for case in self._read(self.named_cases)["cases"]}


def turns(case: dict[str, Any]) -> list[dict[str, Any]]:
    """The calls of `case`, in order: those made on its provider before its
    own, then its own. A case holds the request, the exchanges and the
    outcome of its own call as it holds those of each call before."""
    return [*case.get("before", []), case]


def goes_silent(exchange: dict[str, Any]) -> bool:
    """Whether the fake stops answering this exchange: the provider's
    timeout ends it."""
    response = exchange["response"]
    failed = response.get("failure")
    reads = response.get("chunks")
    return (failed is not None and failed != "connection refused") or (
        reads is not None and response.get("then") not in ENDINGS
    )


def set_apart(case: dict[str, Any]) -> str | None:
    """How `case` is set apart, for what its exchanges are, not by its name:
    None where the fake answers every exchange to its end.

    The provider's timeout ends each silence. The Rust replay lets that
    time pass on a clock it moves; here it passes, so the case is given a
    short timeout in place of its configuration's (`SHORT_TIMEOUTS`): one
    for a case whose one request is never answered (`NEVER_ANSWERED`),
    another for a case whose fake stops answering after it answered
    something, or before a try that is answered (`SILENT_UNDER_WAY`)."""
    exchanges = [exchange for turn in turns(case) for exchange in turn["exchanges"]]
    silences = [goes_silent(exchange) for exchange in exchanges]
    if not any(silences):
        return None
    never = silences == [True] and exchanges[0]["response"].get("failure")
    return NEVER_ANSWERED if never else SILENT_UNDER_WAY


def cases_set_apart(replays: Replays) -> dict[str, set[str]]:
    """The cases of the provider's fixture that are set apart, by how."""
    apart: dict[str, set[str]] = {NEVER_ANSWERED: set(), SILENT_UNDER_WAY: set()}
    for case in replays.fixture()["cases"]:
        how = set_apart(case)
        if how is not None:
            apart[how].add(case["name"])
    return apart


def chunk_bytes(chunk: str | dict[str, str]) -> bytes:
    return chunk.encode() if isinstance(chunk, str) else bytes.fromhex(chunk["hex"])


def reply_of(answered: dict[str, Any]) -> Reply:
    failed = answered.get("failure")
    if failed is not None:
        return Refused() if failed == "connection refused" else Silent()
    status, headers = answered["status"], answered.get("headers", {})
    if answered.get("chunks") is not None:
        reads = [chunk_bytes(chunk) for chunk in answered["chunks"]]
        return Reads(status, headers, reads, ENDINGS.get(answered.get("then"), "silent"))
    if answered.get("text") is not None:
        return Whole(status, headers, answered["text"].encode())
    return Whole(status, headers, json.dumps(answered.get("json")).encode())


def written(call: str, given: Any) -> Any:
    """What a call gave, RoomKit's models, in the fixture's JSON form: a
    response without the tasks and observations RoomKit adds."""
    if call == "generate":
        return given.model_dump(mode="json", exclude={"tasks", "observations"})
    if call == "generate_stream":
        return given
    return [item.model_dump(mode="json") for item in given]


def replaying(replays: Replays, provider: Any, fake: Fake) -> Any:
    """Sends `provider` to `fake`, through the seam the native module keeps
    for this replay: its configuration as written, its clock set to the
    fixtures', and its waits kept instead of taken. Gives those waits."""
    config = provider._config
    form = config.model_dump_json()
    # A configuration that states no key has none to give; one names the
    # member that holds it.
    key = getattr(config, type(config)._key, None)
    provider._native, waits = _native._replaying(
        replays.vendor, form, key, "127.0.0.1", fake.port, CLOCK
    )
    return waits


@dataclass
class Called:
    """What one call of a replayed case gave."""

    # The key the call's request carries for itself, when it carries one: a
    # text that is not empty.
    request_key: str | None = None
    # Why the contract did not read the call's request, when it did not.
    unread: str | None = None
    # What the call returned, in the fixture's JSON form, then the failure
    # that ended it, and whether that failure is a response schema's.
    given: Any = None
    error: Exception | None = None
    schema: bool = False
    asked: bool = False
    seen: list[Seen] = field(default_factory=list)
    waits: list[float] = field(default_factory=list)


@dataclass
class Replayed(Called):
    """What replaying a case gave: its own call, and what holds for every
    call of the case."""

    # The key of the case's configuration.
    key: str = ""
    # Why the provider was not built, when it was not.
    unbuilt: Exception | None = None
    # What the provider answers of itself, for each such member of the case.
    supports: dict[str, Any] = field(default_factory=dict)
    # The calls made on the provider before the case's own, in order.
    before: list[Called] = field(default_factory=list)


def context_of(turn: dict[str, Any]) -> AIContext:
    """The request of one call, with the key it carries for itself where
    its JSON form holds a mask."""
    request = dict(turn["request"])
    if "request_api_key" in turn:
        lent = {API_KEY_METADATA_KEY: turn["request_api_key"]}
        request["metadata"] = {**request.get("metadata", {}), **lent}
    return AIContext.model_validate(request)


async def _call(
    provider: Any, call: str, turn: dict[str, Any], fake: Fake, waits: Any, called: Called
) -> None:
    """Makes one call of a case on `provider`: `turn` holds its request and
    its exchanges, which `fake` answers with. What it gave is kept in
    `called`."""
    lent = turn.get("request_api_key")
    called.request_key = lent if isinstance(lent, str) and lent else None
    context = context_of(turn)
    fake.answer(*(reply_of(exchange["response"]) for exchange in turn["exchanges"]))
    # The waits of the calls made before are theirs.
    kept = len(waits.taken())
    called.asked = True
    # What a stream gave before it failed is kept too.
    items: list[Any] | None = None
    try:
        if call == "generate":
            called.given = written(call, await provider.generate(context))
        elif call == "list_models":
            called.given = written(call, await provider.list_models())
        else:
            stream = (
                provider.generate_stream(context)
                if call == "generate_stream"
                else provider.generate_structured_stream(context)
            )
            items = []
            async for item in stream:
                items.append(item)
    except ProviderError as error:
        called.error = error
        called.schema = isinstance(error, ResponseSchemaError)
        # A request the contract refuses fails as one a provider refuses
        # before it sends it: it is known by the package's words.
        if error.before_request and said(error).startswith(UNREAD):
            called.unread = said(error).removeprefix(UNREAD)
    if items is not None:
        called.given = written(call, items)
    called.waits = waits.taken()[kept:]


async def replay(
    replays: Replays, case: dict[str, Any], fake: Fake, *, read_timeout: float | None = None
) -> Replayed:
    """Replays `case`: its provider built from its configuration, its
    requests sent to `fake`, which answers with the case's exchanges, its
    clock set to the fixtures' and its waits kept instead of taken. The
    calls the case records before its own are made first, on the same
    provider, each answered with its own exchanges.

    `read_timeout` replaces the configuration's timeout, for a case whose
    fake goes silent: nothing else ends a silence."""
    config = dict(case["config"])
    if replays.prepared is not None:
        config = replays.prepared(config)
    if read_timeout is not None:
        config["timeout"] = read_timeout
    replayed = Replayed(key=config.get("api_key") or "")
    try:
        provider = replays.provider(replays.config(**config))
    except ProviderError as error:
        replayed.unbuilt = error
        return replayed
    replayed.supports = {
        name: getattr(provider, name) for name in case if name.startswith(SUPPORTS)
    }
    waits = replaying(replays, provider, fake)
    for turn in case.get("before", []):
        earlier = Called()
        await _call(provider, case["call"], turn, fake, waits, earlier)
        # Its answers written, what the fake saw of them is whole.
        await fake.served()
        earlier.seen = list(fake.seen)
        replayed.before.append(earlier)
    await _call(provider, case["call"], case, fake, waits, replayed)
    # The provider gone, every connection is closed, and the fake ends
    # serving each: what it saw then says how it wrote each answer.
    del provider
    await fake.served()
    replayed.seen = list(fake.seen)
    return replayed


def said(error: Exception) -> str:
    """What a failure says: its first argument, without the provider and
    the status `str()` leads it with."""
    return error.args[0]


def _text(value: Any) -> str:
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False)


def _as_rust(value: str | None) -> str:
    """A text that may be absent, as the Rust replay writes one: a named
    case states a header it sends otherwise in those words."""
    return "None" if value is None else f"Some({json.dumps(value, ensure_ascii=False)})"


def _body_of(seen: bytes, sent: Any) -> Any:
    """The body `seen`, as the fixture writes the one Python sent: JSON, or,
    where the fixture writes a form (``{"form": [[name, value], ...]}``), its
    pairs decoded, in order."""
    if not seen:
        return None
    if isinstance(sent, dict) and "form" in sent:
        return {"form": [list(pair) for pair in parse_qsl(seen.decode())]}
    return json.loads(seen)


def _requests_differ(turn: dict[str, Any], key: str, called: Called, found: list[str]) -> None:
    exchanges = turn["exchanges"]
    if len(called.seen) != len(exchanges):
        found.append(f"{len(called.seen)} requests sent, where Python sent {len(exchanges)}")
    for at, (seen, exchange) in enumerate(zip(called.seen, exchanges, strict=False)):
        sent = exchange["request"]
        url = urlsplit(sent["url"])
        target = url.path + (f"?{url.query}" if url.query else "")
        if (seen.method, seen.target) != (sent["method"], target):
            found.append(f"request {at}: {seen.method} {seen.target}")
        body = _body_of(seen.body, sent.get("body"))
        if _text(body) != _text(sent.get("body")):
            found.append(f"request {at}: its body is {_text(body)}")
        _headers_differ(at, seen, sent, (key, called.request_key), found)
        recorded = exchange["response"].get("chunks")
        reads = sum(1 for chunk in recorded or [] if chunk_bytes(chunk))
        # A try fails where its call asks again after it, or ends in a
        # failure after the last.
        failed = at + 1 < len(called.seen) or called.error is not None
        _reads_differ(at, seen, reads, failed, found)


def _reads_differ(at: int, seen: Seen, recorded: int, failed: bool, found: list[str]) -> None:
    """Where the answer to request `at` was not served in the `recorded`
    reads, or was left by a try that did not fail: `failed` says whether the
    try of that request did.

    A provider that fails on a read closes its connection, and the fake may
    still be writing the reads after it: which of the two is first is no
    difference from Python. So the fake says what it saw (`Seen.reads`,
    `Seen.left`), and each thing it may have seen is held to its own rule.

    - The client stayed until the reads were written: they are the recorded
      ones, exactly.
    - The client left before: it read less than the answer, which a try that
      failed may do and no other, and the reads written until then are fewer
      than the recorded ones.

    A provider ends a stream it answers from at the stream's own end mark,
    as Python does, and reads nothing a case records after it. No case
    records more than one read there, and that one is written whichever
    side is first: its client leaves once it has the read before."""
    if not seen.left:
        if seen.reads != recorded:
            found.append(
                f"request {at}: its answer was served in {seen.reads} reads, "
                f"where the case records {recorded}"
            )
        return
    if not failed:
        found.append(
            f"request {at}: its answer was left after {seen.reads} reads of {recorded}, "
            f"by a try that did not fail"
        )
    if seen.reads >= recorded:
        found.append(
            f"request {at}: its answer was left with a read to write after {seen.reads}, "
            f"where the case records {recorded}"
        )


def _keyed(value: str, keys: tuple[str, str | None]) -> str:
    """The value of a recorded header, with the key it names in its place:
    the configuration's, or the one the request carries for itself."""
    key, request_key = keys
    value = value.replace(CONFIG_KEY, key)
    return value if request_key is None else value.replace(REQUEST_KEY, request_key)


def _headers_differ(
    at: int, seen: Seen, sent: dict[str, Any], keys: tuple[str, str | None], found: list[str]
) -> None:
    own_agent = "user-agent" not in sent["headers"]
    if own_agent and seen.headers.get("user-agent") != USER_AGENT:
        found.append(f"request {at}: user-agent is {_as_rust(seen.headers.get('user-agent'))}")
    for name, value in sent["headers"].items():
        expected = None if value is None else _keyed(value, keys)
        if seen.headers.get(name) != expected:
            found.append(f"request {at}: {name} is {_as_rust(seen.headers.get(name))}")
    names = [name for name, _ in seen.lines]
    for name in names:
        if names.count(name) != 1:
            found.append(f"request {at}: {name} is sent {names.count(name)} times")
        if name not in sent["headers"] and name not in (*TRANSPORT_HEADERS, "user-agent"):
            found.append(f"request {at}: {name} is sent, which Python does not send")


def _waits_differ(turn: dict[str, Any], called: Called, found: list[str]) -> None:
    expected = [exchange["wait"] for exchange in turn["exchanges"] if exchange.get("wait")]
    if len(expected) != len(called.waits):
        found.append(f"{len(called.waits)} waits, where Python took {len(expected)}")
    for taken, wait in zip(called.waits, expected, strict=False):
        most = wait["seconds"]
        least = most if wait["from"] == "the answer's headers" else most * 0.75
        if taken < least - 1e-6 or taken > most + 1e-6:
            found.append(f"a wait of {taken} s, where Python took {most} s")


def _supports_differ(case: dict[str, Any], replayed: Replayed) -> list[str]:
    """Where what the provider answers of itself is not what Python's
    answers: each such member the case records."""
    return [
        f"{name} is {replayed.supports.get(name)}"
        for name, stated in case.items()
        if name.startswith(SUPPORTS) and replayed.supports.get(name) != stated
    ]


def _shaped_as(given: str, minted: str) -> bool:
    """Whether `given` has the shape of an id Python minted as `minted`:
    ``call_<name>_<12 hexadecimal digits>``, for the same name."""
    name, _, drawn = given.rpartition("_")
    shaped = len(drawn) == 12 and all(digit in "0123456789abcdefABCDEF" for digit in drawn)
    return shaped and minted.rpartition("_")[0] == name


def _name_minted_ids(turn: dict[str, Any], expected: Any, given: Any, held: dict[str, str]) -> Any:
    """`given` with the ids Python minted written where it holds an id of
    the same shape in the same place: such an id is random, so its shape is
    compared, and that the items of one call share one id, not its value."""
    if isinstance(expected, list) and isinstance(given, list):
        named = [_name_minted_ids(turn, e, g, held) for e, g in zip(expected, given, strict=False)]
        return named + given[len(named) :]
    if not (isinstance(expected, dict) and isinstance(given, dict)):
        return given
    named = dict(given)
    for name, value in expected.items():
        if name not in given:
            continue
        minted = name == "id" and value in turn.get("minted_call_ids", [])
        if not (minted and isinstance(given[name], str)):
            named[name] = _name_minted_ids(turn, value, given[name], held)
            continue
        # One id of the provider's stands for one of Python's, and for no
        # other.
        stands = held.setdefault(value, given[name])
        one_for_one = stands == given[name] and all(
            other == value or theirs != given[name] for other, theirs in held.items()
        )
        if _shaped_as(given[name], value) and one_for_one:
            named[name] = value
    return named


def _result_differs(
    turn: dict[str, Any], what: str, expected: Any, given: Any, found: list[str]
) -> None:
    if expected is None:
        found.append(f"{what} where Python gives none: {_text(given)}")
        return
    given = _name_minted_ids(turn, expected, given, {})
    if _text(given) != _text(expected):
        found.append(f"{what}: {_text(given)}")


def _failure_differs(
    turn: dict[str, Any], failure: dict[str, Any], called: Called, found: list[str]
) -> None:
    error: Any = called.error
    compare = turn.get("compare")
    if compare is None:
        found.append("a failure with no rule to compare it")
        return
    if not compare["exactly"]:
        found.append(f"a {failure['class']} no provider reproduces")
        return
    same = {
        "class": (failure["class"] == "ResponseSchemaError") == called.schema,
        "status_code": failure.get("status_code") == error.status_code,
        "retryable": failure.get("retryable") == error.retryable,
        "provider": failure.get("provider") == error.provider,
        "context_overflow": failure.get("context_overflow") == error.context_overflow,
        "before_request": failure.get("before_request") == error.before_request,
        "reason": failure.get("reason") == getattr(error, "reason", None),
        # What a stream gave before it failed is compared with its items.
        "events": True,
        "text": True,
    }
    for attribute in compare["exactly"]:
        if not same[attribute]:
            found.append(f"the failure's {attribute} differs: {error!r}")
    if compare["message"] in ("roomkit", "server") and said(error) != failure["message"]:
        found.append(f"the failure says {said(error)!r}")


def _end_differs(turn: dict[str, Any], called: Called, found: list[str]) -> None:
    failure, error = turn["expected"].get("error"), called.error
    if error is not None and failure is not None:
        _failure_differs(turn, failure, called, found)
    elif error is not None:
        found.append(f"a failure where Python answers: {error!r}")
    elif failure is not None:
        found.append(f"an answer where Python fails: {failure['message']}")


def _call_differs(
    call: str, turn: dict[str, Any], key: str, called: Called, supports: list[str]
) -> list[str]:
    """Every way one call differs from Python's: `called`, what it gave,
    against `turn`, what the fixture records of it. `supports` is where the
    provider answers of itself otherwise than Python's, which stands where
    the Rust replay finds it: after the requests and the waits."""
    if called.unread is not None:
        return [f"the request is not read: {called.unread}"]
    found: list[str] = []
    _requests_differ(turn, key, called, found)
    _waits_differ(turn, called, found)
    found += supports
    if not called.asked:
        return [*found, "nothing was asked"]
    expected = turn["expected"]
    what, member = {
        "generate": ("the response", "response"),
        "list_models": ("the models", "models"),
        "generate_stream": ("the text", "text"),
        "generate_structured_stream": ("the events", "events"),
    }[call]
    # What a stream gave before it failed is compared too; a whole call
    # that failed gave nothing.
    if called.given is not None:
        _result_differs(turn, what, expected.get(member), called.given, found)
    _end_differs(turn, called, found)
    return found


def differences(case: dict[str, Any], replayed: Replayed) -> list[str]:
    """Every way `replayed` differs from what Python did of `case`; none
    when the provider replays the case as Python."""
    if replayed.unbuilt is not None:
        return [f"the provider is not built: {said(replayed.unbuilt)}"]
    found: list[str] = []
    before = case.get("before", [])
    if len(replayed.before) != len(before):
        found.append(
            f"{len(replayed.before)} calls made before the case's own, "
            f"where Python made {len(before)}"
        )
    for at, (turn, called) in enumerate(zip(before, replayed.before, strict=False)):
        earlier = _call_differs(case["call"], turn, replayed.key, called, [])
        found += [f"call {at} before the case's own: {difference}" for difference in earlier]
    supports = _supports_differ(case, replayed)
    return found + _call_differs(case["call"], case, replayed.key, replayed, supports)


def _says(words: str, says: dict[str, str]) -> bool:
    """Whether `words` are what an entry states a failure says: these words
    and no other, or a reader's words that hold these."""
    return words == says["exactly"] if "exactly" in says else says["holding"] in words


def _fails(error: Any, does: dict[str, Any]) -> bool:
    return (
        error is not None
        and _says(said(error), does["says"])
        and error.status_code == does["status_code"]
        and error.retryable == does["retryable"]
        and error.context_overflow == does["context_overflow"]
        and not error.before_request
    )


def _answers(does: dict[str, Any], call: str, given: Any) -> bool:
    """Whether a call that was answered, whole or as a stream that ended
    without a failure, said the text, stopped for the reason and counted the
    usage an entry states."""
    if call == "generate":
        text, finish, usage = given["content"], given["finish_reason"], given["usage"]
    elif call == "generate_structured_stream":
        done = next((event for event in given if event["type"] == "done"), None)
        if done is None:
            return False
        text = "".join(event["text"] for event in given if event["type"] == "text_delta")
        finish, usage = done["finish_reason"], done["usage"]
    else:
        return False
    return (text, finish, _text(usage)) == (
        does["text"],
        does["finish_reason"],
        _text(does["usage"]),
    )


def _is_done(
    replays: Replays, stated: dict[str, Any], case: dict[str, Any], replayed: Replayed
) -> bool:
    """Whether `replayed` did exactly what the entry of a named case says."""
    if len(replayed.seen) != stated["requests"]:
        return False
    does = stated["does"]
    kind = does["kind"]
    if kind == "unbuilt":
        error = replayed.unbuilt
        return (
            error is not None
            and error.before_request
            and not error.retryable
            and said(error) == does["says"]
        )
    if replayed.unbuilt is not None:
        return False
    if kind == "unread":
        # Nothing is asked: the failure is one of before the request, which
        # asking again does not change.
        error: Any = replayed.error
        unread = replayed.unread
        refused = unread is not None and error is not None
        return refused and _says(unread, does["says"]) and not error.retryable
    if kind == "fails":
        return _fails(replayed.error, does)
    if kind == "fails_before":
        # A host's source of tokens gave none: final, before the request to
        # the provider's endpoint, no status.
        error = replayed.error
        return (
            error is not None
            and error.before_request
            and not error.retryable
            and error.status_code is None
            and _says(said(error), does["says"])
        )
    if kind == "asks_without_a_key":
        nothing = {
            "says": {"exactly": "the provider answered HTTP 599: no message"},
            "status_code": 599,
            "retryable": True,
            "context_overflow": None,
        }
        no_key = bool(replayed.seen) and replays.key_header not in replayed.seen[0].headers
        return case["call"] == "generate" and no_key and _fails(replayed.error, nothing)
    if kind != "answers" or replayed.error is not None:
        return False
    return _answers(does, case["call"], replayed.given)


def judged(
    replays: Replays, case: dict[str, Any], stated: dict[str, Any] | None, replayed: Replayed
) -> tuple[bool, str]:
    """How a replayed case stands: ``(True, "")`` as Python, ``(False, "")``
    as its entry among the named cases states, else what is wrong."""
    differing = differences(case, replayed)
    if stated is None:
        return True, "; ".join(differing)
    if not differing:
        return False, "named, and it replays as Python"
    as_stated = len(differing) == len(stated["differs"]) and all(
        found.startswith(prefix) for found, prefix in zip(differing, stated["differs"], strict=True)
    )
    if not as_stated:
        return False, (
            f"it differs from Python otherwise than its entry states, "
            f"{stated['differs']}: {differing}"
        )
    if not _is_done(replays, stated, case, replayed):
        gave = (replayed.unread, replayed.unbuilt, replayed.given, replayed.error)
        return False, f"not {stated['does']}: {gave!r}"
    return False, ""


async def every_case(replays: Replays) -> tuple[dict[tuple[str, str], int], list[str]]:
    """Replays every case of the provider's fixture, one after the other on
    one fake. Gives how many cases of each call stand each way (as Python,
    or named and as their entry states), and what is wrong, case by case: a
    difference from Python of a case that is not named, a named case that
    replays as Python, or one that does not differ or do as its entry
    states."""
    cases, stated = replays.fixture()["cases"], replays.named()
    counts: Counter[tuple[str, str]] = Counter()
    wrong = []
    async with Fake() as fake:
        for case in cases:
            name, call = case["name"], case["call"]
            short = SHORT_TIMEOUTS.get(set_apart(case))
            replayed = await replay(replays, case, fake, read_timeout=short)
            as_python, told = judged(replays, case, stated.get(name), replayed)
            if told:
                wrong.append(f"{name}: {told}")
            counts[call, "as Python" if as_python else "named"] += 1
    return dict(counts), wrong


async def made_wrong(
    replays: Replays,
    name: str,
    wrong: Callable[[Replayed], None] | None = None,
    *,
    in_case: Callable[[dict[str, Any]], None] | None = None,
    in_entry: Callable[[dict[str, Any]], None] | None = None,
    entry_of: str | None = None,
) -> str:
    """What the replay says of the case `name` once one thing of it is made
    wrong: of what was replayed (`wrong`), of the case (`in_case`) or of
    its entry among the named cases (`in_entry`). The case is replayed and
    must first stand as stated, as Python or as its entry. `entry_of` holds
    it to the entry of another case instead, which is the thing made wrong."""
    cases = {case["name"]: case for case in replays.fixture()["cases"]}
    case = copy.deepcopy(cases[name])
    stated = copy.deepcopy(replays.named().get(entry_of or name))
    async with Fake() as fake:
        replayed = await replay(replays, case, fake)
    if entry_of is None:
        told = judged(replays, case, stated, replayed)[1]
        if told:
            raise AssertionError(f"{name} does not replay as stated: {told}")
    for change, changed in ((wrong, replayed), (in_case, case), (in_entry, stated)):
        if change is not None:
            change(changed)
    return judged(replays, case, stated, replayed)[1]
