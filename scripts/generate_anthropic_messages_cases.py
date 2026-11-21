#!/usr/bin/env python3
"""Write what RoomKit's Python Anthropic provider sends and reads, case by case.

Each case runs ``AnthropicAIProvider`` once, on the Messages API or on the
model listing, against a fake of the wire (nothing leaves this script), and
records:

- ``config``: the keyword arguments of its ``AnthropicConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads), and ``request_api_key`` when the request
  carries a key of its own, which the form writes masked;
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised, and ``compare``, how a replay compares that error;
- ``before``, on a case that needs them: the calls made on the same provider
  before the case's own, each written as the case's own is.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

The provider takes no transport and builds a client of the anthropic SDK per
key, so the fake sits under every client at once (``under_every_client``):
nothing of how the provider or the SDK builds a client is changed.

Run it with a Python interpreter that has ``roomkit`` and its ``anthropic``
extra installed (``make python-anthropic-messages-cases``). The fixture
records the versions and the commit of ``roomkit`` it was written from.
"""

from __future__ import annotations

import asyncio
import json
import warnings
from collections.abc import Iterator
from contextlib import contextmanager
from functools import partial
from pathlib import Path
from typing import Any

import anthropic
import httpx2
from anthropic import _base_client as sdk
from anthropic_messages_cases import CASES, REQUEST_KEYS
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.ai.reasoning import reasoning_floor
from roomkit.providers.anthropic import AnthropicAIProvider, AnthropicConfig
from wire_capture import (
    KEY,
    Case,
    MintedIds,
    Reply,
    Turn,
    Wire,
    answered_texts,
    case_form,
    data_lines,
    fixture_of,
    named_key,
    outcome,
    prepare,
    request_form,
    turn_form,
    unique_names,
    write,
)

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = (
    REPOSITORY / "crates/roomkit-providers-anthropic/tests/fixtures/python_messages_cases.json"
)

# The tags of a model's catalogue entry the provider reads.
READ_TAGS = (
    "deferred_tools",
    "effort_refused",
    "effort_no_xhigh",
    "effort_up_to_high",
    "thinking_off_disabled",
    "thinking_off_between_tools",
    "thinking_off_effort_high",
)

# The headers of a request the fixture keeps beside the configured ones.
KEPT_HEADERS = ("x-api-key", "authorization", "anthropic-version", "content-type", "accept")

# The classes the anthropic SDK raises for an answer whose status is an error.
STATUS_ERRORS = {
    "APIStatusError",
    "BadRequestError",
    "AuthenticationError",
    "PermissionDeniedError",
    "NotFoundError",
    "ConflictError",
    "RequestTooLargeError",
    "UnprocessableEntityError",
    "RateLimitError",
    "InternalServerError",
    "ServiceUnavailableError",
    "DeadlineExceededError",
    "OverloadedError",
}

# The classes of the anthropic SDK a call may raise as they are.
SDK_ERRORS = {
    "AnthropicError",
    "APIConnectionError",
    "APITimeoutError",
    "APIResponseValidationError",
    *STATUS_ERRORS,
}

# The classes the HTTP client raises for a body it could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

# How the anthropic SDK's own sentences start, where it raises a Python
# exception of no class of its own.
SDK_SENTENCES = (
    "Unable to parse tool parameter JSON from model.",
    "Unexpected event order, got ",
    "Unexpected event runtime type, ",
    '"Could not resolve authentication method.',
)

NOTES = {
    "environment": (
        "Every ANTHROPIC_* variable of the environment is removed before a case runs: the "
        "anthropic SDK reads a base URL, default headers and more from them when a client "
        "is given none. No case is captured with one set."
    ),
    "versions": (
        "The versions the fixture was written from. A message in Python's own words, which "
        "no replay compares, may name one: pydantic's names its version in a link."
    ),
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether "
        "the package's files had changes not committed then. Both are null unless the "
        "package's own files are tracked by the repository that holds them: a package "
        "installed inside some checkout is not that checkout's. catalogue holds what the "
        "provider read of its model's catalogue entry, which it reads whatever the "
        "endpoint: whether the catalogue knows the model, the tags among those the "
        "provider reads, and the least reasoning effort the entry declares. A case that "
        "changes because the catalogue changed shows it there."
    ),
    "config": "The keyword arguments of AnthropicConfig. The key is a placeholder.",
    "supports": (
        "supports_vision and supports_deferred_tools are what the provider of the case "
        "answers: whether its model takes images, and whether it can hold a tool declared "
        "and unseen. supports_streaming, supports_structured_streaming, "
        "supports_response_schema and supports_response_schema_with_tools are true for "
        "every configuration."
    ),
    "request": (
        "AIContext dumped with model_dump(mode='json'), its ten request fields. A key the "
        "request carries is written masked there, and plain under request_api_key."
    ),
    "before": (
        "The calls made on the provider of the case before the case's own, in order, each "
        "with its request, its request_api_key, its exchanges and what it gave. The case's "
        "own request, exchanges and expected are those of its last call, and stand alone: "
        "a replay that makes the last call only reads the same."
    ),
    "exchanges.request": (
        "What Python sent: the method, the whole URL (scheme, host, a port that is written, "
        "path and query), the body as JSON with its members in the order sent, and five "
        "headers with each configured extra header: x-api-key and authorization, with a "
        "key replaced by which key it was (<config key> or <request key>), "
        "anthropic-version, content-type and accept. A header that was not sent is null. "
        "RoomKit decides the key and the extra headers; the anthropic SDK decides the "
        "three others, which the endpoint reads. The other headers of the SDK and of the "
        "platform are left out. timeouts holds, in seconds, how long the request may wait "
        "to connect, for a read, for a write and for a connection of the pool. Every "
        "generation is asked as a stream: generate and generate_stream read the same "
        "stream generate_structured_stream gives."
    ),
    "exchanges.response": (
        "What the fake answered: status and headers with a body under json, under text for "
        "a body that is no JSON, or under chunks for a body read in several reads. Each "
        "chunk is the bytes of one read, in order: a text where the bytes are UTF-8, written "
        "as that text, else an object whose hex member holds the bytes in hexadecimal, as a "
        "read that ends inside a character is. then says how the reads end after them: the "
        "stream ends, the connection is lost, or the read times out. A failure in place of a "
        "response is under failure: no answer came."
    ),
    "exchanges.wait": (
        "The wait the anthropic SDK took before its next try, in seconds, and where it came "
        "from: the answer's headers, or the SDK's backoff, which is up to a quarter less "
        "than what is written. An HTTP date is read against clock."
    ),
    "expected": (
        "What Python returned: response for generate (AIResponse without tasks and "
        "observations), text for generate_stream (the deltas), events for "
        "generate_structured_stream (each event dumped), models for list_models (each "
        "ModelInfo dumped). A failure is under error, after "
        "whatever a stream gave first: its class, its message (the first argument), its text "
        "(str), the attributes of a ProviderError, and raised_from, the class of the SDK or "
        "transport error it was raised from, if any."
    ),
    "compare": (
        "On a call that fails, how a replay compares the failure. exactly lists what it "
        "reproduces as written: attributes of the error, reason among them where the error "
        "is a response schema's, and events or text, what the stream gave before it "
        "failed. An empty list is a failure no provider reproduces: the "
        "SDK's own, or a raw Python exception. message says whose words the message holds, "
        "and so whether it is compared: roomkit (RoomKit's own words: compared), server (the "
        "text the server sent, handed on as it came: compared), sdk (the anthropic SDK's "
        "words: not compared), python_json (what Python's JSON reader says of a text: not "
        "compared), python_repr (Python's repr of the JSON the server sent, of an error body "
        "after 'Error code: <status> - ' and of a stream's error event alone: not compared), "
        "transport (the HTTP client's words for a body it could not read to its end: not "
        "compared), python (the words of a Python exception raised while the answer was "
        "read: not compared). A python_json message "
        "leaves RoomKit's own words before it uncompared too ('the answer is not a JSON "
        "document (...)'). A compared roomkit message may hold Python's repr of a name or "
        "of a list (tool 'orders.lookup', missing ['city']), which a replay writes the same "
        "way."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call its server gave no id of its "
        "own: call_<name>_<12 hexadecimal digits>, random. A replay compares their shape, "
        "and that the events of one call share one id, not their value."
    ),
    "sdk": (
        "Where a case says sdk, the anthropic SDK decided the behaviour, not RoomKit: its "
        "retries (which answers, how many times, after what wait), how it joins a URL, "
        "merges a body or follows a redirect, how it reads a stream's events, and the words "
        "of a failure that has no answer. A provider without that SDK decides these itself."
    ),
    "questioned": (
        "Where a case says questioned, it pins a behaviour of Python that is not settled, "
        "in the sentence given, which starts with its kind. 'A possible defect': what looks "
        "like a defect of Python, to report upstream. 'A difference to decide': a point "
        "where the Rust provider is planned otherwise, which the maintainer decides; no "
        "defect is claimed. A replay may set the case aside, or expect otherwise once it is "
        "settled. The fixture lists their names under questioned."
    ),
}


@contextmanager
def under_every_client(wire: Wire) -> Iterator[None]:
    """While it lasts, ``wire`` answers every request an httpx2 client would
    put on the network, and each wait the SDK computes before a next try is
    recorded and not taken.

    The fake replaces the one method of httpx2's network transport that sends
    a request, for every transport there is: the provider's configured client
    and each client it builds for a request's own key are built as they are
    against the real endpoint, and all of them answer from ``wire``.
    """
    send = httpx2.AsyncHTTPTransport.handle_async_request

    async def answered(transport: httpx2.AsyncHTTPTransport, request: httpx2.Request) -> Any:
        await request.aread()
        return await wire(request)

    httpx2.AsyncHTTPTransport.handle_async_request = answered
    wire.record_waits(anthropic.AsyncAnthropic, sdk)
    try:
        yield
    finally:
        httpx2.AsyncHTTPTransport.handle_async_request = send
        # The recorder was set on the class itself, over the method it inherits.
        del anthropic.AsyncAnthropic._calculate_retry_timeout


def sent(request: httpx2.Request, case: Case, request_key: Any) -> dict[str, Any]:
    """``request`` as the fixture writes it; ``request_key`` is the key the
    call's own request carries, if it is one."""
    own = request_key if isinstance(request_key, str) and request_key else None
    headers = {name: request.headers.get(name) for name in KEPT_HEADERS}
    for name in ("x-api-key", "authorization"):
        headers[name] = named_key(headers[name], own)
    for name in case.config.get("extra_headers") or {}:
        if name.lower() not in headers:
            headers[name.lower()] = request.headers.get(name)
    return request_form(request, headers)


def event_reprs(replies: tuple[Reply, ...]) -> list[str]:
    """Python's repr of each JSON document a ``data`` line of ``replies``
    holds: the words the SDK raises a stream's error event with."""
    reprs = []
    for line in data_lines(replies):
        try:
            reprs.append(str(json.loads(line)))
        except ValueError:
            continue
    return reprs


def words_of(error: dict[str, Any], replies: tuple[Reply, ...]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``);
    ``replies`` are what the fake answered the call with."""
    cause, message = error["raised_from"], error["message"]
    if error["class"] in SDK_ERRORS:
        return "sdk"
    if "provider" not in error:
        return "python"
    if cause in ("APIConnectionError", "APITimeoutError"):
        return "sdk"
    if cause == "JSONDecodeError":
        return "python_json"
    if cause in TRANSPORT_ERRORS:
        return "transport"
    if message.startswith("Error code: "):
        return "python_repr" if " - " in message else "sdk"
    if message in event_reprs(replies):
        return "python_repr"
    served = [*answered_texts(replies), *(line.strip() for line in data_lines(replies))]
    if message in served:
        return "server"
    if message.startswith(SDK_SENTENCES):
        return "sdk"
    return "roomkit" if cause in (None, "ValueError") else "python"


def told_by(provider: AnthropicAIProvider) -> dict[str, Any]:
    """What ``provider`` says of itself: what it reads of its model's
    catalogue entry, and what it supports that depends on it."""
    entry = provider.catalog_entry()
    tags = [tag for tag in READ_TAGS if entry is not None and tag in entry.capabilities]
    catalogue = {
        "model": provider.model_name,
        "known": entry is not None,
        "tags": tags,
        "reasoning_floor": reasoning_floor(entry),
    }
    return {
        "catalogue": catalogue,
        "supports_vision": provider.supports_vision,
        "supports_deferred_tools": provider.supports_deferred_tools,
    }


async def run_turn(
    provider: AnthropicAIProvider, case: Case, turn: Turn, minted: MintedIds
) -> dict[str, Any]:
    """One call of ``case`` on its provider, as the fixture writes it."""
    request_key = turn.request.get("metadata", {}).get("api_key")
    wire = Wire(case.name, turn.replies, partial(sent, case=case, request_key=request_key))
    context = AIContext.model_validate(turn.request)
    with under_every_client(wire):
        expected = await outcome(provider, case.call, context)
    words = partial(words_of, replies=turn.replies)
    return turn_form(turn.request, context, wire, expected, words, minted)


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    minted = tool_calls.uuid4 = MintedIds()
    provider = AnthropicAIProvider(AnthropicConfig(**case.config))
    try:
        turns = [
            await run_turn(provider, case, turn, minted)
            for turn in (*case.before, Turn(case.request, case.replies))
        ]
        return case_form(case, told_by(provider), turns)
    finally:
        await provider.close()


def keys_stay_out_of_errors(cases: list[dict[str, Any]]) -> None:
    """Refuse a fixture whose expected error holds a key."""
    for case in cases:
        for turn in (*case.get("before", []), case):
            if KEY in json.dumps(turn["expected"].get("error", {})):
                raise AssertionError(f"{case['name']}: the error it expects holds a key")


def main() -> None:
    # The SDK reads a base URL, default headers and more from the environment
    # when a client is given none: a case is what its configuration says.
    prepare(sdk, "ANTHROPIC_")
    # The SDK dumps an event whose member is of another type than its model
    # says, and pydantic warns of it: a warning is no part of what a case returns.
    warnings.filterwarnings("ignore", message="Pydantic serializer warnings")
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    keys_stay_out_of_errors(cases)
    generator = "scripts/generate_anthropic_messages_cases.py"
    fixture = fixture_of(generator, anthropic, NOTES, cases)
    write(FIXTURE, REPOSITORY, fixture, anthropic, REQUEST_KEYS)


if __name__ == "__main__":
    main()
