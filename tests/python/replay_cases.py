"""The replay of the recorded wire cases through the Python package: what a
case's provider sent and gave, and where that differs from what Python did.

A case is asked as RoomKit asks it: its configuration is the package's
``OpenAIConfig``, its request an ``AIContext``, and what the provider gives
are RoomKit's models, written back in the JSON form the fixture holds; a
failure is RoomKit's own exception.

The comparison is the Rust replay's (``crates/roomkit-providers-openai``,
``src/replay``), in its words, so that the cases the provider answers
otherwise than Python are held to the same statement: the data file the Rust
crate writes of them, ``named_cases.json``.
"""

from __future__ import annotations

import json
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

from fake_endpoint import Fake, Reads, Refused, Reply, Seen, Silent, Whole
from roomkit.providers.ai import AIContext, ProviderError, ResponseSchemaError
from roomkit_providers import _native
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "crates/roomkit-providers-openai/tests/fixtures"

# The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
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

# How a recorded stream of reads ends, as the fake serves it.
ENDINGS = {"the stream ends": "ends", "the connection is lost": "lost"}


def fixture() -> dict[str, Any]:
    return json.loads((FIXTURES / "python_chat_cases.json").read_text(encoding="utf-8"))


def named() -> dict[str, dict[str, Any]]:
    """The cases the provider answers otherwise than Python, by name, as the
    Rust replay states them."""
    held = json.loads((FIXTURES / "named_cases.json").read_text(encoding="utf-8"))
    return {case["name"]: case for case in held["cases"]}


def goes_silent(exchange: dict[str, Any]) -> bool:
    """Whether the fake stops answering this exchange: the provider's
    timeout ends it."""
    response = exchange["response"]
    failed = response.get("failure")
    reads = response.get("chunks")
    return (failed is not None and failed != "connection refused") or (
        reads is not None and response.get("then") not in ENDINGS
    )


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


def replaying(provider: OpenAI, fake: Fake) -> Any:
    """Sends `provider` to `fake`, through the seam the native module keeps
    for this replay: its configuration as written, its clock set to the
    fixture's, and its waits kept instead of taken. Gives those waits."""
    config = provider._config
    key = config.api_key.get_secret_value()
    form = config.model_dump_json()
    provider._native, waits = _native._openai_replaying(form, key, "127.0.0.1", fake.port, CLOCK)
    return waits


@dataclass
class Replayed:
    """What replaying a case gave."""

    # The key of the case's configuration.
    key: str = ""
    # Why the provider was not built, when it was not.
    unbuilt: Exception | None = None
    # What the call returned, in the fixture's JSON form, then the failure
    # that ended it, and whether that failure is a response schema's.
    given: Any = None
    error: Exception | None = None
    schema: bool = False
    asked: bool = False
    seen: list[Seen] = field(default_factory=list)
    waits: list[float] = field(default_factory=list)
    vision: bool | None = None


async def replay(
    case: dict[str, Any], fake: Fake, *, read_timeout: float | None = None
) -> Replayed:
    """Replays `case`: its provider built from its configuration, its
    requests sent to `fake`, which answers with the case's exchanges, its
    clock set to the fixture's and its waits kept instead of taken.

    `read_timeout` replaces the configuration's timeout, for a case whose
    fake never answers: nothing else ends it."""
    config = dict(case["config"])
    if read_timeout is not None:
        config["timeout"] = read_timeout
    replayed = Replayed(key=config["api_key"])
    try:
        provider = OpenAI(OpenAIConfig(**config))
    except ProviderError as error:
        replayed.unbuilt = error
        return replayed
    replayed.vision = provider.supports_vision
    fake.answer(*(reply_of(exchange["response"]) for exchange in case["exchanges"]))
    waits = replaying(provider, fake)
    context, call = AIContext.model_validate(case["request"]), case["call"]
    replayed.asked = True
    # What a stream gave before it failed is kept too.
    items: list[Any] | None = None
    try:
        if call == "generate":
            replayed.given = written(call, await provider.generate(context))
        elif call == "list_models":
            replayed.given = written(call, await provider.list_models())
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
        replayed.error = error
        replayed.schema = isinstance(error, ResponseSchemaError)
    if items is not None:
        replayed.given = written(call, items)
    replayed.waits = waits.taken()
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


def _requests_differ(case: dict[str, Any], replayed: Replayed, found: list[str]) -> None:
    exchanges = case["exchanges"]
    if len(replayed.seen) != len(exchanges):
        found.append(f"{len(replayed.seen)} requests sent, where Python sent {len(exchanges)}")
    for at, (seen, exchange) in enumerate(zip(replayed.seen, exchanges, strict=False)):
        sent = exchange["request"]
        url = urlsplit(sent["url"])
        target = url.path + (f"?{url.query}" if url.query else "")
        if (seen.method, seen.target) != (sent["method"], target):
            found.append(f"request {at}: {seen.method} {seen.target}")
        body = json.loads(seen.body) if seen.body else None
        if _text(body) != _text(sent.get("body")):
            found.append(f"request {at}: its body is {_text(body)}")
        _headers_differ(at, seen, sent, replayed.key, found)
        recorded = exchange["response"].get("chunks")
        reads = sum(1 for chunk in recorded or [] if chunk_bytes(chunk))
        if seen.reads != reads:
            found.append(
                f"request {at}: its answer was served in {seen.reads} reads, "
                f"where the case records {reads}"
            )


def _headers_differ(at: int, seen: Seen, sent: dict[str, Any], key: str, found: list[str]) -> None:
    own_agent = "user-agent" not in sent["headers"]
    if own_agent and seen.headers.get("user-agent") != USER_AGENT:
        found.append(f"request {at}: user-agent is {seen.headers.get('user-agent')!r}")
    for name, value in sent["headers"].items():
        expected = None if value is None else value.replace("Bearer <config key>", f"Bearer {key}")
        if seen.headers.get(name) != expected:
            found.append(f"request {at}: {name} is {seen.headers.get(name)!r}")
    names = [name for name, _ in seen.lines]
    for name in names:
        if names.count(name) != 1:
            found.append(f"request {at}: {name} is sent {names.count(name)} times")
        if name not in sent["headers"] and name not in (*TRANSPORT_HEADERS, "user-agent"):
            found.append(f"request {at}: {name} is sent, which Python does not send")


def _waits_differ(case: dict[str, Any], replayed: Replayed, found: list[str]) -> None:
    expected = [exchange["wait"] for exchange in case["exchanges"] if exchange.get("wait")]
    if len(expected) != len(replayed.waits):
        found.append(f"{len(replayed.waits)} waits, where Python took {len(expected)}")
    for taken, wait in zip(replayed.waits, expected, strict=False):
        most = wait["seconds"]
        least = most if wait["from"] == "the answer's headers" else most * 0.75
        if taken < least - 1e-6 or taken > most + 1e-6:
            found.append(f"a wait of {taken} s, where Python took {most} s")


def _shaped_as(given: str, minted: str) -> bool:
    """Whether `given` has the shape of an id Python minted as `minted`:
    ``call_<name>_<12 hexadecimal digits>``, for the same name."""
    name, _, drawn = given.rpartition("_")
    shaped = len(drawn) == 12 and all(digit in "0123456789abcdefABCDEF" for digit in drawn)
    return shaped and minted.rpartition("_")[0] == name


def _name_minted_ids(case: dict[str, Any], expected: Any, given: Any, held: dict[str, str]) -> Any:
    """`given` with the ids Python minted written where it holds an id of
    the same shape in the same place: such an id is random, so its shape is
    compared, and that the items of one call share one id, not its value."""
    if isinstance(expected, list) and isinstance(given, list):
        named = [_name_minted_ids(case, e, g, held) for e, g in zip(expected, given, strict=False)]
        return named + given[len(named) :]
    if not (isinstance(expected, dict) and isinstance(given, dict)):
        return given
    named = dict(given)
    for name, value in expected.items():
        if name not in given:
            continue
        minted = name == "id" and value in case.get("minted_call_ids", [])
        if not (minted and isinstance(given[name], str)):
            named[name] = _name_minted_ids(case, value, given[name], held)
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
    case: dict[str, Any], what: str, expected: Any, given: Any, found: list[str]
) -> None:
    if expected is None:
        found.append(f"{what} where Python gives none: {_text(given)}")
        return
    given = _name_minted_ids(case, expected, given, {})
    if _text(given) != _text(expected):
        found.append(f"{what}: {_text(given)}")


def _failure_differs(
    case: dict[str, Any], failure: dict[str, Any], replayed: Replayed, found: list[str]
) -> None:
    error: Any = replayed.error
    compare = case.get("compare")
    if compare is None:
        found.append("a failure with no rule to compare it")
        return
    if not compare["exactly"]:
        found.append(f"a {failure['class']} no provider reproduces")
        return
    same = {
        "class": (failure["class"] == "ResponseSchemaError") == replayed.schema,
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


def _end_differs(case: dict[str, Any], replayed: Replayed, found: list[str]) -> None:
    failure, error = case["expected"].get("error"), replayed.error
    if error is not None and failure is not None:
        _failure_differs(case, failure, replayed, found)
    elif error is not None:
        found.append(f"a failure where Python answers: {error!r}")
    elif failure is not None:
        found.append(f"an answer where Python fails: {failure['message']}")


def differences(case: dict[str, Any], replayed: Replayed) -> list[str]:
    """Every way `replayed` differs from what Python did of `case`; none
    when the provider replays the case as Python."""
    if replayed.unbuilt is not None:
        return [f"the provider is not built: {said(replayed.unbuilt)}"]
    found: list[str] = []
    _requests_differ(case, replayed, found)
    _waits_differ(case, replayed, found)
    stated = case.get("supports_vision")
    if stated is not None and replayed.vision != stated:
        found.append(f"supports_vision is {replayed.vision}")
    if not replayed.asked:
        return [*found, "nothing was asked"]
    expected = case["expected"]
    what, member = {
        "generate": ("the response", "response"),
        "list_models": ("the models", "models"),
        "generate_stream": ("the text", "text"),
        "generate_structured_stream": ("the events", "events"),
    }[case["call"]]
    # What a stream gave before it failed is compared too; a whole call
    # that failed gave nothing.
    if replayed.given is not None:
        _result_differs(case, what, expected.get(member), replayed.given, found)
    _end_differs(case, replayed, found)
    return found


def _fails(error: Any, does: dict[str, Any]) -> bool:
    if error is None:
        return False
    says = does["says"]
    words = said(error)
    as_stated = words == says["exactly"] if "exactly" in says else says["holding"] in words
    return (
        as_stated
        and error.status_code == does["status_code"]
        and error.retryable == does["retryable"]
        and error.context_overflow == does["context_overflow"]
        and not error.before_request
    )


def _is_done(stated: dict[str, Any], case: dict[str, Any], replayed: Replayed) -> bool:
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
    if kind == "fails":
        return _fails(replayed.error, does)
    if kind == "asks_without_a_key":
        nothing = {
            "says": {"exactly": "the provider answered HTTP 599: no message"},
            "status_code": 599,
            "retryable": True,
            "context_overflow": None,
        }
        no_key = bool(replayed.seen) and "authorization" not in replayed.seen[0].headers
        return case["call"] == "generate" and no_key and _fails(replayed.error, nothing)
    if kind != "answers" or replayed.error is not None:
        return False
    if case["call"] == "generate":
        response = replayed.given
        text, finish, usage = response["content"], response["finish_reason"], response["usage"]
    else:
        events = replayed.given
        done = next((event for event in events if event["type"] == "done"), None)
        if done is None:
            return False
        text = "".join(event["text"] for event in events if event["type"] == "text_delta")
        finish, usage = done["finish_reason"], done["usage"]
    return (text, finish, _text(usage)) == (
        does["text"],
        does["finish_reason"],
        _text(does["usage"]),
    )


def judged(
    case: dict[str, Any], stated: dict[str, Any] | None, replayed: Replayed
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
    if not _is_done(stated, case, replayed):
        return False, f"not {stated['does']}: {replayed.unbuilt!r} {replayed.given!r}"
    return False, ""
