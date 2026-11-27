#!/usr/bin/env python3
"""Write what RoomKit's Python Ollama provider sends and reads, case by case.

Each case runs ``OllamaAIProvider`` once, on Ollama's native ``/api/chat``
(whole, or streamed as JSON lines) or on its listing (``/api/tags``, then
``/api/show`` for each model), against a fake of the wire (nothing leaves
this script), and records:

- ``config``: the keyword arguments of its ``OllamaConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads);
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised, and ``compare``, how a replay compares that error.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

The provider takes no transport: the ollama SDK builds its own httpx client.
The fake sits under every httpx client at once (``under_every_client``):
nothing of how the SDK builds its client is changed.

Run it with a Python interpreter that has ``roomkit`` and its ``ollama``
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

import httpx
import ollama
from ollama import _client as sdk
from ollama_cases import CASES
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.ollama import OllamaAIProvider, OllamaConfig
from wire_capture import (
    KEY,
    Case,
    MintedIds,
    Reply,
    Wire,
    answered_texts,
    case_form,
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
FIXTURE = REPOSITORY / "crates/roomkit-providers-ollama/tests/fixtures/python_ollama_cases.json"

# The headers of a request the fixture keeps, beside those a configuration
# names.
KEPT_HEADERS = ("authorization", "content-type", "accept")

# The classes the ollama SDK raises for an answer whose status is an error,
# or for a line of a stream that holds one.
SDK_ERRORS = {"ResponseError", "RequestError"}

# The classes the HTTP client raises for a body it could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    "environment": (
        "Every OLLAMA_* variable of the environment is removed before a case runs: the ollama "
        "SDK reads a key (OLLAMA_API_KEY) and a host (OLLAMA_HOST) from them. No case is "
        "captured with one set."
    ),
    "versions": "The versions the fixture was written from.",
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether "
        "the package's files had changes not committed then."
    ),
    "config": "The keyword arguments of OllamaConfig. A key is a placeholder.",
    "supports": (
        "supports_vision, supports_streaming, supports_structured_streaming and "
        "supports_response_schema are true for every configuration; "
        "supports_response_schema_with_tools is false."
    ),
    "request": "AIContext dumped with model_dump(mode='json'), its ten request fields.",
    "exchanges.request": (
        "What Python sent: the method, the whole URL (path and query), the body as JSON "
        "with its members in the order sent, and the headers authorization (a key "
        "replaced by <config key>), content-type and accept, with those the configuration "
        "names but user-agent, which the SDK sets over a configured one. A header that "
        "was not sent is null. RoomKit decides the host, the key and "
        "the headers; the ollama SDK decides the URL a host is read as, the paths, the "
        "other headers and how the body is written, but for a request that declares "
        "tools, whose body RoomKit writes (providers/ollama/sdk_patch.py). The other "
        "headers of the SDK are left out. timeouts holds, in seconds, how long the request "
        "may wait to connect, for a read, for a write and for a connection of the pool. "
        "generate asks a whole answer (stream false); generate_stream and "
        "generate_structured_stream ask a stream of JSON lines."
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
        "server (the text the server sent, handed on as it came: compared), sdk (the ollama "
        "SDK's words: not compared), python_json (what Python's JSON reader says of a "
        "text: not compared), transport (the HTTP client's words for a body it could not "
        "read to its end: not compared), python (the words of a Python exception raised "
        "while the answer was read: not compared)."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call: call_<name>_<12 hexadecimal "
        "digits>, random. A replay compares their shape, and that the events of one call "
        "share one id, not their value. Ollama names no call: every id is minted."
    ),
    "sdk": (
        "Where a case says sdk, the ollama SDK decided the behaviour, not RoomKit: the URL "
        "a host is read as, how it writes a body and reads a stream's lines, how it reads "
        "an image, and the words of a failure. A provider without that SDK decides these "
        "itself."
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
    transport that sends a request: the client the SDK builds is built as it
    is against the real endpoint, and answers from ``wire``."""
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
    headers["authorization"] = named_key(headers["authorization"])
    for name in case.config.get("headers") or {}:
        # The SDK sets its own user-agent over a configured one.
        if name.lower() not in headers and name.lower() != "user-agent":
            headers[name.lower()] = request.headers.get(name)
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
    if message in served_texts(replies):
        return "server"
    if cause in SDK_ERRORS:
        return "sdk"
    return "roomkit" if cause in (None, "ValueError") else "python"


def served_texts(replies: tuple[Reply, ...]) -> list[str]:
    """Every text the fake answered with: the bodies, each line of a stream,
    and the error a JSON line or body names."""
    texts = list(answered_texts(replies))
    for reply in replies:
        for chunk in reply.chunks or []:
            text = chunk.decode() if isinstance(chunk, bytes) else chunk
            for read in text.splitlines():
                texts.append(read.strip())
                try:
                    texts.append(str(json.loads(read).get("error")))
                except (ValueError, AttributeError):
                    pass
        if isinstance(reply.body, dict) and "error" in reply.body:
            texts.append(str(reply.body["error"]))
    return texts


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    minted = tool_calls.uuid4 = MintedIds()
    provider = OllamaAIProvider(OllamaConfig(**case.config))
    wire = Wire(case.name, case.replies, partial(sent, case=case), http=httpx)
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
    # The SDK reads a key and a host from the environment: a case is what
    # its configuration says.
    prepare(sdk, "OLLAMA_")
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    keys_stay_out_of_errors(cases)
    generator = "scripts/generate_ollama_cases.py"
    fixture = fixture_of(generator, ollama, NOTES, cases, http=httpx)
    write(FIXTURE, REPOSITORY, fixture, ollama, http=httpx)


if __name__ == "__main__":
    main()
