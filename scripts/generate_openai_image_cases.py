#!/usr/bin/env python3
"""Write what RoomKit's Python OpenAI image provider sends and reads, case by case.

Each case runs ``OpenAIImageProvider`` once, a generation (``images.generate``)
or an edit (``images.edit``), whole or as a stream of previews, against a fake
of the wire (``httpx2.MockTransport``: nothing leaves this script), and
records:

- ``config``: the keyword arguments of its ``OpenAIImageConfig``;
- ``request``: what the image surface was asked (``openai_image_cases``);
- ``exchanges``: each HTTP request Python sent and the response the fake gave
  it; a multipart body is written as its parts;
- ``expected``: what Python returned, the images or the error it raised, and
  every attempt its progress callback was told of; ``compare``, how a replay
  compares the error.

The Rust provider replays the table: the same requests sent, the same images
or the same error given, the same attempts told.

The id of each attempt is drawn by ``uuid4``: here it is drawn in order,
``00000000-0000-4000-8000-000000000001`` first, and a replay names the ids its
provider draws in the order they come.

Run it with a Python interpreter that has ``roomkit`` and its ``openai`` extra
installed. The fixture records the versions and the commit of ``roomkit`` it
was written from.
"""

from __future__ import annotations

import asyncio
import uuid
from functools import partial
from pathlib import Path
from typing import Any

import httpx2
import openai
from openai import _base_client as sdk
from openai_image_cases import CASES
from roomkit.providers.image import base as image_base
from roomkit.providers.openai.config import OpenAIImageConfig
from roomkit.providers.openai.image import OpenAIImageProvider
from wire_capture import (
    EXACT,
    Case,
    Wire,
    answered_texts,
    case_form,
    fixture_of,
    image_outcome,
    named_key,
    prepare,
    request_form,
    unique_names,
    write,
)

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-openai/tests/fixtures/python_image_cases.json"

NOTES = {
    "environment": (
        "Every OPENAI_* variable of the environment is removed before a case runs: the SDK "
        "reads a key and a base URL from it when its client is given none."
    ),
    "versions": "The versions the fixture was written from.",
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether the "
        "package's files had changes not committed then."
    ),
    "config": "The keyword arguments of OpenAIImageConfig. The key is a placeholder.",
    "request": (
        "What the image surface was asked: prompt, size, n, reference_images and mask "
        "(AIImagePart dumps), options (ImageOptions keywords, null for none) and progress: "
        "null for no callback, record for one that records each attempt it is told of, fail "
        "for one that records it, then raises RuntimeError('storage unavailable') when told "
        "of a succeeded one. image_generate calls generate, image_generate_with_options "
        "calls generate_with_options."
    ),
    "exchanges.request": (
        "What Python sent: the method, the whole URL, the body, and three headers: "
        "authorization, the key replaced by <config key>, content-type and accept. A JSON "
        "body is written as JSON, its members in the order sent. A multipart body is written "
        "under multipart as its parts, in order: each its name, a file's file name, media "
        "type and bytes (base64), a field's text; its content type is written without its "
        "boundary, which the client draws at random. timeouts holds, in seconds, how long "
        "the request may wait to connect, for a read, for a write and for a connection of "
        "the pool."
    ),
    "exchanges.response": (
        "What the fake answered: status and headers with a body under json, or under chunks "
        "for a body read in several reads, and then, how the reads end after them. A failure "
        "in place of a response is under failure: no answer came."
    ),
    "expected": (
        "What Python returned: results, each ImageResult dumped, or error: its class, its "
        "message, its text, the attributes of a ProviderError, raised_from, the class of the "
        "error it was raised from, and for an ImageGenerationError its attempts and results, "
        "dumped. progress lists, in order, every attempt the callback was told of, dumped. "
        "An attempt's id is drawn in order from 00000000-0000-4000-8000-000000000001."
    ),
    "compare": (
        "On a call that fails, how a replay compares the failure. exactly lists what it "
        "reproduces as written. message says whose words the message holds: roomkit "
        "(RoomKit's own: compared), server (the text the server sent, handed on: compared), "
        "python_repr (RoomKit's text of the server's body, 'Error code: <status> - ' and "
        "Python's repr of the JSON: not compared), python_json (what Python's JSON reader "
        "says of a text: not compared), transport (the HTTP client's words for a request it "
        "would not write: not compared), sdk (the SDK's words: not compared). An attempt's "
        "error holds the failure's message: where the message is not compared, whether the "
        "attempt has one is."
    ),
}

# The SDK's classes for a status error and for a failed connection.
SDK_ERRORS = {"APIConnectionError", "APITimeoutError", "APIStatusError"}


class DrawnIds:
    """``uuid4`` drawing ids in order: an attempt's id, the same on every
    run."""

    def __init__(self) -> None:
        self.drawn = 0

    def __call__(self) -> uuid.UUID:
        self.drawn += 1
        return uuid.UUID(f"00000000-0000-4000-8000-{self.drawn:012d}")


def sent(request: httpx2.Request) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {
        "authorization": named_key(request.headers.get("authorization")),
        "content-type": request.headers.get("content-type"),
        "accept": request.headers.get("accept"),
    }
    return request_form(request, headers)


def words_of(error: dict[str, Any], replies: list[str]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``)."""
    message, cause = error["message"], error["raised_from"]
    if cause in SDK_ERRORS - {"APIStatusError"}:
        return "sdk"
    if cause == "JSONDecodeError":
        return "python_json"
    if error["class"] == "ImageGenerationError" and cause == "ValueError":
        # A refusal of the HTTP client, raised as the request was written.
        return "transport"
    if message.startswith("Error code: "):
        return "python_repr" if " - " in message else "sdk"
    if message in replies:
        return "server"
    return "roomkit"


def comparison(expected: dict[str, Any], replies: list[str]) -> dict[str, Any] | None:
    """How a replay compares the failure of ``expected``: a refusal before
    any request by its class and its message; a provider's failure by its
    attributes, an image generation's by its attempts and images too."""
    error = expected.get("error")
    if error is None:
        return None
    if "provider" not in error:
        return {"exactly": ["class"], "message": words_of(error, replies)}
    outcomes = ["attempts", "results"] if "attempts" in error else []
    return {"exactly": [*EXACT, *outcomes], "message": words_of(error, replies)}


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    image_base.uuid4 = DrawnIds()
    wire = Wire(case.name, case.replies, sent)
    transport = httpx2.MockTransport(wire)
    provider = OpenAIImageProvider(OpenAIImageConfig(**case.config), transport=transport)
    try:
        expected = await image_outcome(provider, case.call, case.request)
    finally:
        await provider.close()
    wire.all_asked_for()
    turn: dict[str, Any] = {"request": case.request, "exchanges": wire.exchanges, "expected": expected}
    compare = comparison(expected, answered_texts(case.replies))
    if compare is not None:
        turn["compare"] = compare
    return case_form(case, {}, [turn])


def main() -> None:
    prepare(sdk, "OPENAI_")
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    fixture = fixture_of("scripts/generate_openai_image_cases.py", openai, NOTES, cases)
    write(FIXTURE, REPOSITORY, fixture, openai)


if __name__ == "__main__":
    main()
