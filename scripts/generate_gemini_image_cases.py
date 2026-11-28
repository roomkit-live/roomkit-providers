#!/usr/bin/env python3
"""Write what RoomKit's Python Gemini image provider sends and reads, case by case.

Each case runs ``GeminiImageProvider`` once, ``n`` interactions of the
Interactions API (``client.aio.interactions.create``), against a fake of the
wire (nothing leaves this script), and records what ``generate_openai_image_
cases.py`` records: the configuration, the request, each HTTP request and the
answer the fake gave it, and what Python gave, the images or the error with its
attempts and images, and every attempt the progress callback was told of.

The provider takes no transport: the fake sits under every httpx client at
once (``generate_gemini_cases.under_every_client``). The ``n`` interactions of
a request reach the fake in the order they start, and each takes the next
answer.

Run it with a Python interpreter that has ``roomkit`` and its ``gemini`` extra
installed. The fixture records the versions and the commit of ``roomkit`` it
was written from.
"""

from __future__ import annotations

import asyncio
import os
import re
import warnings
from pathlib import Path
from typing import Any

import httpx
from generate_gemini_cases import KEPT_HEADERS, SDK_ERRORS, TRANSPORT_ERRORS, under_every_client
from generate_openai_image_cases import NOTES as IMAGE_NOTES
from generate_openai_image_cases import DrawnIds
from gemini_image_cases import CASES
from google import genai
from google.genai import _api_client as sdk
from roomkit.providers.gemini.config import GeminiImageConfig
from roomkit.providers.gemini.image import GeminiImageProvider
from roomkit.providers.image import base as image_base
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
FIXTURE = REPOSITORY / "crates/roomkit-providers-gemini/tests/fixtures/python_image_cases.json"

NOTES = {
    **IMAGE_NOTES,
    "environment": (
        "Every GOOGLE_* and GEMINI_* variable of the environment is removed before a case "
        "runs: the SDK reads a key from it when its client is given none."
    ),
    "config": "The keyword arguments of GeminiImageConfig. The key is a placeholder.",
    "exchanges.request": (
        "What Python sent: the method, the whole URL, the body as JSON with its members in "
        "the order sent, and three headers: x-goog-api-key, the key replaced by <config "
        "key>, content-type and accept. The n interactions of a request are n requests, in "
        "the order they started. timeouts holds, in seconds, how long the request may wait "
        "to connect, for a read, for a write and for a connection of the pool."
    ),
}


def sent(request: httpx.Request) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {name: request.headers.get(name) for name in KEPT_HEADERS}
    headers["x-goog-api-key"] = named_key(headers["x-goog-api-key"])
    return request_form(request, headers)


def words_of(error: dict[str, Any], replies: list[str]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``). A
    failed generation says what its first failure says, led by the
    provider's name where it names none: the failure's own words then follow
    it."""
    cause, message = error["raised_from"], error["message"]
    if error["class"] in SDK_ERRORS or cause in SDK_ERRORS:
        return "sdk"
    if cause == "JSONDecodeError":
        return "python_json"
    if cause in TRANSPORT_ERRORS:
        return "transport"
    if message in replies:
        return "server"
    said = message.removeprefix("gemini: ")
    if re.match(r"gemini \(\d+\): Error code: \d+ - ", message):
        return "python_repr"
    if said.startswith(("Gemini ", "Image progress callback failed: ")) or cause in (None, "ValueError"):
        return "roomkit"
    # What google-genai and httpx say of a request they could not make or
    # an answer they could not read.
    return "sdk"


def comparison(expected: dict[str, Any], replies: list[str]) -> dict[str, Any] | None:
    """As ``generate_openai_image_cases.comparison``."""
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
    wire = Wire(case.name, case.replies, sent, http=httpx)
    provider = GeminiImageProvider(GeminiImageConfig(**case.config))
    try:
        with under_every_client(wire):
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
    prepare(sdk, "GOOGLE_")
    for name in [name for name in os.environ if name.startswith("GEMINI_")]:
        del os.environ[name]
    warnings.filterwarnings("ignore", category=SyntaxWarning)
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    generator = "scripts/generate_gemini_image_cases.py"
    fixture = fixture_of(generator, genai, NOTES, cases, http=httpx)
    write(FIXTURE, REPOSITORY, fixture, genai, http=httpx)


if __name__ == "__main__":
    main()
