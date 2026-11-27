#!/usr/bin/env python3
"""Write what RoomKit's Python Mistral provider sends and reads, case by case.

Each case runs ``MistralAIProvider`` once, on Mistral's chat completions
(always streamed) or on its model listing, against a fake of the wire
(nothing leaves this script), and records:

- ``config``: the keyword arguments of its ``MistralConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads);
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised, and ``compare``, how a replay compares that error.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

The provider takes no transport: the mistralai SDK builds its own httpx2
client. The fake sits under every httpx2 client at once
(``under_every_client``): nothing of how the SDK builds its client is
changed.

Run it with a Python interpreter that has ``roomkit`` and its ``mistral``
extra installed. The fixture records the versions and the commit of
``roomkit`` it was written from.
"""

from __future__ import annotations

import asyncio
import json
from collections.abc import Iterator
from contextlib import contextmanager
from functools import partial
from pathlib import Path
from typing import Any

import httpx2
import mistralai
from mistral_cases import CASES
from mistralai.client.utils import retries as sdk
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.mistral import MistralAIProvider, MistralConfig
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
FIXTURE = REPOSITORY / "crates/roomkit-providers-mistral/tests/fixtures/python_mistral_cases.json"

# The headers of a request the fixture keeps.
KEPT_HEADERS = ("authorization", "content-type", "accept")

# The classes the mistralai SDK raises for an answer whose status is an
# error, or for a body it cannot read.
SDK_ERRORS = {
    "SDKError",
    "HTTPValidationError",
    "MistralError",
    "NoResponseError",
    "ResponseValidationError",
}

# The classes the HTTP client raises for a body it could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    "environment": (
        "Every MISTRAL_* variable of the environment is removed before a case runs: the "
        "mistralai SDK reads a key and more from them. No case is captured with one set."
    ),
    "versions": "The versions the fixture was written from.",
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether "
        "the package's files had changes not committed then."
    ),
    "config": "The keyword arguments of MistralConfig. The key is a placeholder.",
    "supports": (
        "supports_vision, supports_streaming, supports_structured_streaming and "
        "supports_response_schema are true for every configuration; "
        "supports_response_schema_with_tools is false."
    ),
    "request": "AIContext dumped with model_dump(mode='json'), its ten request fields.",
    "exchanges.request": (
        "What Python sent: the method, the whole URL (path and query) as it goes on the "
        "wire, without the '#stream' fragment the SDK names its operation with, the body "
        "as JSON with its members in the order sent, and three headers: authorization, the key "
        "replaced by <config key>, content-type and accept. A header that was not sent is "
        "null. RoomKit decides the key and the base URL; the mistralai SDK decides the "
        "path, the two other headers and how the body is written. The other headers of "
        "the SDK are left out. timeouts holds, in seconds, how long the request may wait "
        "to connect, for a read, for a write and for a connection of the pool: the SDK's "
        "client's, which RoomKit does not set. Every generation is asked as a stream: "
        "generate and generate_stream read the same stream generate_structured_stream "
        "gives."
    ),
    "exchanges.response": (
        "What the fake answered: status and headers with a body under json, under text for "
        "a body that is no JSON, or under chunks for a body read in several reads. Each "
        "chunk is the bytes of one read, in order. then says how the reads end after them: "
        "the stream ends, the connection is lost, or the read times out. A failure in place "
        "of a response is under failure: no answer came."
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
        "server (the text the server sent, handed on as it came: compared), sdk (the "
        "mistralai SDK's words: not compared), python_json (what Python's JSON reader says "
        "of a text: not compared), transport (the HTTP client's words for a body it could "
        "not read to its end: not compared), python (the words of a Python exception "
        "raised while the answer was read: not compared)."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call its server gave no id of its "
        "own: call_<name>_<12 hexadecimal digits>, random. A replay compares their shape, "
        "and that the events of one call share one id, not their value."
    ),
    "sdk": (
        "Where a case says sdk, the mistralai SDK decided the behaviour, not RoomKit: the "
        "path it asks, how it writes a body and reads a stream's events, its timeouts, and "
        "the words of a failure. A provider without that SDK decides these itself."
    ),
    "questioned": (
        "Where a case says questioned, it pins a behaviour of Python that is not settled, "
        "in the sentence given, which starts with its kind. The fixture lists their names "
        "under questioned."
    ),
}


@contextmanager
def under_every_client(wire: Wire) -> Iterator[None]:
    """While it lasts, ``wire`` answers every request an httpx2 client would
    put on the network. The fake replaces the one method of httpx2's network
    transport that sends a request: the client the SDK builds is built as it
    is against the real endpoint, and answers from ``wire``."""
    send = httpx2.AsyncHTTPTransport.handle_async_request

    async def answered(transport: httpx2.AsyncHTTPTransport, request: httpx2.Request) -> Any:
        await request.aread()
        return await wire(request)

    httpx2.AsyncHTTPTransport.handle_async_request = answered
    try:
        yield
    finally:
        httpx2.AsyncHTTPTransport.handle_async_request = send


def sent(request: httpx2.Request) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {name: request.headers.get(name) for name in KEPT_HEADERS}
    headers["authorization"] = named_key(headers["authorization"])
    written = request_form(request, headers)
    # The SDK names its operation in the URL's fragment, which no request
    # carries on the wire.
    written["url"] = str(request.url.copy_with(fragment=None))
    return written


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
    minted = tool_calls.uuid4 = MintedIds()
    provider = MistralAIProvider(MistralConfig(**case.config))
    wire = Wire(case.name, case.replies, sent, http=httpx2)
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
    # The SDK reads a key and more from the environment: a case is what its
    # configuration says.
    prepare(sdk, "MISTRAL_")
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    keys_stay_out_of_errors(cases)
    generator = "scripts/generate_mistral_cases.py"
    fixture = fixture_of(generator, mistralai, NOTES, cases, http=httpx2)
    write(FIXTURE, REPOSITORY, fixture, mistralai, http=httpx2)


if __name__ == "__main__":
    main()
