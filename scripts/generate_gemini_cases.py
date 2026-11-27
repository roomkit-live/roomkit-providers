#!/usr/bin/env python3
"""Write what RoomKit's Python Gemini provider sends and reads, case by case.

Each case runs ``GeminiAIProvider`` once, on the Developer API's
``streamGenerateContent`` or on its model listing, against a fake of the
wire (nothing leaves this script), and records:

- ``config``: the keyword arguments of its ``GeminiConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads);
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised, and ``compare``, how a replay compares that error;
- ``before``, on a case that needs them: the calls made on the same provider
  before the case's own, each written as the case's own is.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

The provider takes no transport: it builds the httpx client the google-genai
SDK runs on. The fake sits under every httpx client at once
(``under_every_client``): nothing of how the provider or the SDK builds its
client is changed.

Run it with a Python interpreter that has ``roomkit`` and its ``gemini``
extra installed. The fixture records the versions and the commit of
``roomkit`` it was written from.
"""

from __future__ import annotations

import asyncio
import json
import os
import warnings
from collections.abc import Iterator
from contextlib import contextmanager
from functools import partial
from pathlib import Path
from typing import Any

import httpx
from gemini_cases import CASES
from google import genai
from google.genai import _api_client as sdk
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.gemini import GeminiAIProvider, GeminiConfig
from roomkit.providers.gemini.models import CANNOT_STOP, TAKES_LEVELS, TAKES_MINIMAL
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
FIXTURE = REPOSITORY / "crates/roomkit-providers-gemini/tests/fixtures/python_gemini_cases.json"

# The tags of a model's catalogue entry the provider reads.
READ_TAGS = (TAKES_LEVELS, TAKES_MINIMAL, CANNOT_STOP)

# The headers of a request the fixture keeps.
KEPT_HEADERS = ("x-goog-api-key", "content-type", "accept")

# The classes google-genai raises for an answer whose status is an error, or
# for a body it cannot read.
SDK_ERRORS = {"APIError", "ClientError", "ServerError", "UnknownApiResponseError"}

# The classes the HTTP client raises for a body it could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    "environment": (
        "Every GOOGLE_* and GEMINI_* variable of the environment is removed before a case "
        "runs: the google-genai SDK reads a key, a base URL and more from them. No case is "
        "captured with one set."
    ),
    "versions": "The versions the fixture was written from.",
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether "
        "the package's files had changes not committed then. catalogue holds what the "
        "provider read of its model's catalogue entry: whether the catalogue knows the "
        "model, and the tags among those the provider reads (the thinking levels the model "
        "takes, minimal among them, and whether it cannot stop reasoning)."
    ),
    "config": "The keyword arguments of GeminiConfig. The key is a placeholder.",
    "supports": (
        "supports_vision is what the provider of the case answers. supports_streaming, "
        "supports_structured_streaming, supports_response_schema and "
        "supports_response_schema_with_tools are true for every configuration."
    ),
    "request": "AIContext dumped with model_dump(mode='json'), its ten request fields.",
    "before": (
        "The calls made on the provider of the case before the case's own, in order, each "
        "with its request, its exchanges and what it gave."
    ),
    "exchanges.request": (
        "What Python sent: the method, the whole URL (path and query), the body as JSON "
        "with its members in the order sent, and three headers: x-goog-api-key, the key "
        "replaced by <config key>, content-type and accept. A header that was not sent is "
        "null. RoomKit decides the key; the google-genai SDK decides the URL, the two other "
        "headers and how the body is written. The other headers of the SDK are left out. "
        "timeouts holds, in seconds, how long the request may wait to connect, for a "
        "read, for a write and for a connection of the pool. Every generation is asked as "
        "a stream (alt=sse): generate and generate_stream read the same stream "
        "generate_structured_stream gives."
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
        "google-genai SDK's words: not compared), python_json (what Python's JSON reader "
        "says of a text: not compared), transport (the HTTP client's words for a body it "
        "could not read to its end: not compared), python (the words of a Python "
        "exception raised while the answer was read: not compared)."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call its server gave no id of its "
        "own: call_<name>_<12 hexadecimal digits>, random. A replay compares their shape, "
        "and that the events of one call share one id, not their value."
    ),
    "sdk": (
        "Where a case says sdk, the google-genai SDK decided the behaviour, not RoomKit: "
        "the URL it asks, how it writes a body and reads a stream's lines, and the words of "
        "a failure that has no answer. A provider without that SDK decides these itself."
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
    put on the network. The fake replaces the one method of httpx's network
    transport that sends a request: the client the provider builds for the
    SDK is built as it is against the real endpoint, and answers from
    ``wire``."""
    send = httpx.AsyncHTTPTransport.handle_async_request

    async def answered(transport: httpx.AsyncHTTPTransport, request: httpx.Request) -> Any:
        await request.aread()
        return await wire(request)

    httpx.AsyncHTTPTransport.handle_async_request = answered
    try:
        yield
    finally:
        httpx.AsyncHTTPTransport.handle_async_request = send


def sent(request: httpx.Request, case: Case) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {name: request.headers.get(name) for name in KEPT_HEADERS}
    headers["x-goog-api-key"] = named_key(headers["x-goog-api-key"])
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


def told_by(provider: GeminiAIProvider) -> dict[str, Any]:
    """What ``provider`` says of itself: what it reads of its model's
    catalogue entry, and whether it takes images."""
    entry = provider.catalog_entry()
    tags = [tag for tag in READ_TAGS if entry is not None and tag in entry.capabilities]
    catalogue = {"model": provider.model_name, "known": entry is not None, "tags": tags}
    return {"catalogue": catalogue, "supports_vision": provider.supports_vision}


async def run_turn(
    provider: GeminiAIProvider, case: Case, turn: Turn, minted: MintedIds
) -> dict[str, Any]:
    """One call of ``case`` on its provider, as the fixture writes it."""
    wire = Wire(case.name, turn.replies, partial(sent, case=case), http=httpx)
    context = AIContext.model_validate(turn.request)
    with under_every_client(wire):
        expected = await outcome(provider, case.call, context)
    words = partial(words_of, replies=turn.replies)
    return turn_form(turn.request, context, wire, expected, words, minted)


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    minted = tool_calls.uuid4 = MintedIds()
    provider = GeminiAIProvider(GeminiConfig(**case.config))
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
    # The SDK reads a key, a base URL and more from the environment when a
    # client is given none: a case is what its configuration says.
    prepare(sdk, "GOOGLE_")
    for name in [name for name in os.environ if name.startswith("GEMINI_")]:
        del os.environ[name]
    warnings.filterwarnings("ignore", category=SyntaxWarning)
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    keys_stay_out_of_errors(cases)
    generator = "scripts/generate_gemini_cases.py"
    fixture = fixture_of(generator, genai, NOTES, cases, http=httpx)
    write(FIXTURE, REPOSITORY, fixture, genai, http=httpx)


if __name__ == "__main__":
    main()
