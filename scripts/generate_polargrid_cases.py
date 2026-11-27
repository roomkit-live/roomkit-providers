#!/usr/bin/env python3
"""Write what RoomKit's Python PolarGrid provider sends and reads, case by case.

Each case runs ``PolarGridAIProvider`` once, on an edge's chat completions
(whole or streamed) or on its model listing, against a fake of the wire
(nothing leaves this script), and records:

- ``config``: the keyword arguments of its ``PolarGridConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads);
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it, the autorouter's among them where no region is pinned;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised, and ``compare``, how a replay compares that error.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

The provider takes no transport: polargrid-sdk builds an httpx client for
each request. The fake sits under every httpx client at once
(``under_every_client``). The SDK's cache of the autorouter's pick, which
lives as long as the process, is emptied before each case; the waits of its
retries are recorded and none is taken.

Run it with a Python interpreter that has ``roomkit`` and its ``polargrid``
extra installed. The fixture records the versions and the commit of
``roomkit`` it was written from.
"""

from __future__ import annotations

import asyncio
import json
import uuid
from collections.abc import Iterator
from contextlib import contextmanager
from functools import partial
from pathlib import Path
from typing import Any

import httpx
import polargrid
from polargrid import client as sdk
from polargrid_cases import CASES
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.polargrid import PolarGridAIProvider, PolarGridConfig
from wire_capture import (
    KEY,
    Case,
    MintedIds,
    Reply,
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
FIXTURES = REPOSITORY / "crates/roomkit-providers-polargrid/tests/fixtures"
FIXTURE = FIXTURES / "python_polargrid_cases.json"

# The headers of a request the fixture keeps.
KEPT_HEADERS = ("authorization", "content-type", "accept")

# The classes polargrid-sdk raises for an answer whose status is an error,
# for a request it refuses itself, or for a lost connection.
SDK_ERRORS = {
    "PolarGridError",
    "AuthenticationError",
    "BillingError",
    "ValidationError",
    "RateLimitError",
    "NetworkError",
    "TimeoutError",
    "NotFoundError",
    "ServerError",
}

# The classes the HTTP client raises for a body it could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    "environment": (
        "Every POLARGRID_* variable of the environment is removed before a case runs: the "
        "SDK reads a key from POLARGRID_API_KEY when it is given none. No case is captured "
        "with one set."
    ),
    "versions": "The versions the fixture was written from.",
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether "
        "the package's files had changes not committed then."
    ),
    "config": "The keyword arguments of PolarGridConfig. The key is a placeholder.",
    "supports": (
        "supports_vision is what the provider of the case answers (its model's catalogue "
        "entry). supports_streaming, supports_structured_streaming and "
        "supports_response_schema are true for every configuration; "
        "supports_response_schema_with_tools is false."
    ),
    "request": "AIContext dumped with model_dump(mode='json'), its ten request fields.",
    "exchanges.request": (
        "What Python sent: the method, the whole URL (path and query), the body as JSON with "
        "its members in the order sent, and three headers: authorization, the key replaced "
        "by <config key>, content-type and accept. A header that was not sent is null. The "
        "SDK also sends its user-agent and an X-Request-ID it draws at random for each "
        "request: neither is kept. RoomKit decides the key, the region and the body's "
        "values; the SDK decides the URL, the autorouter's request and how a body is "
        "written. timeouts holds, in seconds, how long the request may wait to connect, for "
        "a read, for a write and for a connection of the pool. Where no region is pinned, "
        "the first request asks the autorouter which edge to use."
    ),
    "exchanges.response": (
        "What the fake answered: status and headers with a body under json, under text for "
        "a body that is no JSON, or under chunks for a body read in several reads. Each "
        "chunk is the bytes of one read, in order. then says how the reads end after them: "
        "the stream ends, the connection is lost, or the read times out. A failure in place "
        "of a response is under failure: no answer came."
    ),
    "exchanges.wait": (
        "Where the SDK tried a request again, the wait it took first, in seconds, its "
        "random part drawn as none, and where it came from: the SDK's backoff, which no "
        "answer's header decides."
    ),
    "expected": (
        "What Python returned: response for generate (AIResponse without tasks and "
        "observations), text for generate_stream (the deltas), events for "
        "generate_structured_stream (each event dumped), models for list_models (each "
        "ModelInfo dumped). A failure is under error, after whatever a stream gave first: "
        "its class, its message (the first argument), its text (str), the attributes of a "
        "ProviderError, and raised_from, the class of the SDK or transport error it was "
        "raised from, if any."
    ),
    "compare": (
        "On a call that fails, how a replay compares the failure. exactly lists what it "
        "reproduces as written: attributes of the error, and events or text, what the "
        "stream gave before it failed. An empty list is a failure no provider reproduces: "
        "the SDK's own, or a raw Python exception. message says whose words the message "
        "holds, and so whether it is compared: roomkit (RoomKit's own words: compared), "
        "server (the text the server sent, handed on as it came: compared), sdk (the SDK's "
        "words: not compared), python_json (what Python's JSON reader says of a text: not "
        "compared), transport (the HTTP client's words for a body it could not read to its "
        "end: not compared), python (the words of a Python exception raised while the "
        "answer was read: not compared)."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call its server gave no id of its "
        "own: call_<name>_<12 hexadecimal digits>, random. A replay compares their shape, "
        "and that the events of one call share one id, not their value."
    ),
    "sdk": (
        "Where a case says sdk, polargrid-sdk decided the behaviour, not RoomKit: the "
        "autorouter's request and its fallback, the URL of an edge, how it writes a body, "
        "reads a stream, tries a request again, and the words of a failure. A provider "
        "without that SDK decides these itself."
    ),
    "questioned": (
        "Where a case says questioned, it pins a behaviour of Python that is not settled, "
        "in the sentence given, which starts with its kind. The fixture lists their names "
        "under questioned."
    ),
}


@contextmanager
def under_every_client(wire: Wire) -> Iterator[None]:
    """While it lasts, ``wire`` answers every request an httpx client would
    put on the network, the SDK's waits between tries are recorded, not
    taken, and the request id its failures quote is drawn as zeros, so that
    a capture writes the same fixture each time."""
    send = httpx.AsyncHTTPTransport.handle_async_request
    asyncio_of_the_sdk, random_of_the_sdk = sdk.asyncio, sdk.random
    uuid_of_the_sdk = sdk.uuid

    async def answered(transport: httpx.AsyncHTTPTransport, request: httpx.Request) -> Any:
        await request.aread()
        return await wire(request)

    class Waits:
        """The SDK's asyncio, its waits recorded and not taken."""

        async def sleep(self, seconds: float) -> None:
            wire.exchanges[-1]["wait"] = {"seconds": seconds, "from": "the SDK's backoff"}

        def __getattr__(self, name: str) -> Any:
            return getattr(asyncio_of_the_sdk, name)

    class Drawn:
        """The SDK's random, its jitter drawn as none."""

        def random(self) -> float:
            return 0.0

        def __getattr__(self, name: str) -> Any:
            return getattr(random_of_the_sdk, name)

    class Zeros:
        """The SDK's uuid, each id drawn as zeros."""

        def uuid4(self) -> uuid.UUID:
            return uuid.UUID(int=0)

        def __getattr__(self, name: str) -> Any:
            return getattr(uuid_of_the_sdk, name)

    httpx.AsyncHTTPTransport.handle_async_request = answered
    sdk.asyncio = Waits()
    sdk.random = Drawn()
    sdk.uuid = Zeros()
    try:
        yield
    finally:
        httpx.AsyncHTTPTransport.handle_async_request = send
        sdk.asyncio, sdk.random = asyncio_of_the_sdk, random_of_the_sdk
        sdk.uuid = uuid_of_the_sdk


def sent(request: httpx.Request) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {name: request.headers.get(name) for name in KEPT_HEADERS}
    headers["authorization"] = named_key(headers["authorization"])
    return request_form(request, headers)


def words_of(error: dict[str, Any], replies: tuple[Reply, ...]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``);
    ``replies`` are what the fake answered the call with."""
    cause, message = error["raised_from"], error["message"]
    if error["class"] in SDK_ERRORS:
        return "sdk"
    if "provider" not in error:
        return "python"
    if cause == "JSONDecodeError":
        return "python_json"
    if cause in TRANSPORT_ERRORS:
        return "transport"
    served = [*answered_texts(replies), *(line.strip() for line in data_lines(replies))]
    if message in served:
        return "server"
    if cause in SDK_ERRORS:
        return "sdk"
    return "roomkit" if cause in (None, "ValueError") else "python"


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    sdk._REGION_CACHE.clear()
    minted = tool_calls.uuid4 = MintedIds()
    provider = PolarGridAIProvider(PolarGridConfig(**case.config))
    wire = Wire(case.name, case.replies, sent, http=httpx)
    context = AIContext.model_validate(case.request)
    try:
        with under_every_client(wire):
            expected = await outcome(provider, case.call, context)
        told = {"supports_vision": provider.supports_vision}
    finally:
        await provider.close()
    words = partial(words_of, replies=case.replies)
    turn = turn_form(case.request, context, wire, expected, words, minted)
    return case_form(case, told, [turn])


def keys_stay_out_of_errors(cases: list[dict[str, Any]]) -> None:
    """Refuse a fixture whose expected error holds a key."""
    for case in cases:
        if KEY in json.dumps(case["expected"].get("error", {})):
            raise AssertionError(f"{case['name']}: the error it expects holds a key")


def main() -> None:
    # The SDK reads a key from the environment when it is given none: a case
    # is what its configuration says.
    prepare(sdk, "POLARGRID_")
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    keys_stay_out_of_errors(cases)
    generator = "scripts/generate_polargrid_cases.py"
    fixture = fixture_of(generator, polargrid, NOTES, cases, http=httpx)
    write(FIXTURE, REPOSITORY, fixture, polargrid, http=httpx)


if __name__ == "__main__":
    main()
