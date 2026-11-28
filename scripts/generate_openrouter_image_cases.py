#!/usr/bin/env python3
"""Write what RoomKit's Python OpenRouter image provider sends and reads, case by case.

Each case runs ``OpenRouterImageProvider`` once, ``n`` requests to OpenRouter's
Image API (``POST /api/v1/images``), against a fake of the wire
(``httpx.MockTransport``: nothing leaves this script), and records what
``generate_openai_image_cases.py`` records: the configuration, the request,
each HTTP request and the answer the fake gave it, and what Python gave, the
images or the error.

The ``n`` requests of a request reach the fake in the order they start, and
each takes the next answer.

Run it with a Python interpreter that has ``roomkit`` and its ``openrouter``
extra installed. The fixture records the versions and the commit of
``roomkit`` it was written from.
"""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

import httpx
from generate_openai_image_cases import NOTES as IMAGE_NOTES
from generate_openai_image_cases import DrawnIds
from openrouter_image_cases import CASES
from roomkit.providers.image import base as image_base
from roomkit.providers.openrouter.config import OpenRouterImageConfig
from roomkit.providers.openrouter.image import OpenRouterImageProvider
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
FIXTURES = REPOSITORY / "crates/roomkit-providers-openai/tests/fixtures"
FIXTURE = FIXTURES / "python_openrouter_image_cases.json"

# The headers the provider decides: the key, the body's type, what it
# accepts, OpenRouter's attribution and a configured one.
KEPT_HEADERS = ("authorization", "content-type", "accept", "http-referer", "x-title", "x-team")

# The classes httpx raises for a request it could not make or an answer it
# could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    **IMAGE_NOTES,
    "environment": (
        "Every OPENROUTER_* variable of the environment is removed before a case runs."
    ),
    "config": "The keyword arguments of OpenRouterImageConfig. The key is a placeholder.",
    "exchanges.request": (
        "What Python sent: the method, the whole URL, the body as JSON with its members in "
        "the order sent, and the headers the provider decides: authorization, the key "
        "replaced by <config key>, content-type, accept, http-referer, x-title and x-team, "
        "each null where it was not sent. The n requests of a request are n requests, in "
        "the order they started. timeouts holds, in seconds, how long the request may wait "
        "to connect, for a read, for a write and for a connection of the pool."
    ),
}


def sent(request: httpx.Request) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {name: request.headers.get(name) for name in KEPT_HEADERS}
    headers["authorization"] = named_key(headers["authorization"])
    return request_form(request, headers)


def words_of(error: dict[str, Any], replies: list[str]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``)."""
    cause, message = error["raised_from"], error["message"]
    if cause == "JSONDecodeError":
        return "python_json"
    if cause in TRANSPORT_ERRORS:
        return "transport"
    if message in replies:
        return "server"
    # RoomKit's words, a refusal's with the server's body after them.
    return "roomkit"


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
    transport = httpx.MockTransport(wire)
    provider = OpenRouterImageProvider(OpenRouterImageConfig(**case.config), transport=transport)
    try:
        expected = await image_outcome(provider, case.call, case.request)
    finally:
        await provider.close()
    wire.all_asked_for()
    turn: dict[str, Any] = {
        "request": case.request,
        "exchanges": wire.exchanges,
        "expected": expected,
    }
    compare = comparison(expected, answered_texts(case.replies))
    if compare is not None:
        turn["compare"] = compare
    return case_form(case, {}, [turn])


def main() -> None:
    prepare(httpx, "OPENROUTER_")
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    generator = "scripts/generate_openrouter_image_cases.py"
    fixture = fixture_of(generator, httpx, NOTES, cases, http=httpx)
    write(FIXTURE, REPOSITORY, fixture, httpx, http=httpx)


if __name__ == "__main__":
    main()
