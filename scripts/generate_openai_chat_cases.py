#!/usr/bin/env python3
"""Write what RoomKit's Python OpenAI provider sends and reads, case by case.

Each case runs ``OpenAIAIProvider`` once, on Chat Completions, against a fake
of the wire (``httpx2.MockTransport``: nothing leaves this script), and
records:

- ``config``: the keyword arguments of its ``OpenAIConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads), and ``request_api_key`` when the request
  carries a key of its own, which the form writes masked;
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

Run it with a Python interpreter that has ``roomkit`` and its ``openai``
extra installed (``make python-openai-chat-cases``). The fixture records the
versions it was written from.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import platform
import time
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import httpx2
import openai
import roomkit
from openai import _base_client as sdk
from openai_chat_cases import CASES, KEY, REQUEST_KEY, Case, Reply
from roomkit.providers.ai import AIContext, ProviderError, tool_calls
from roomkit.providers.openai import OpenAIAIProvider, OpenAIConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-openai/tests/fixtures/python_chat_cases.json"

# The fields of ``AIContext`` a provider reads: the contract's request.
REQUEST_FIELDS = {
    "messages",
    "system_prompt",
    "temperature",
    "max_tokens",
    "thinking_budget",
    "enable_thinking",
    "reasoning_effort",
    "tools",
    "response_schema",
    "metadata",
}

# The fields of ``AIResponse`` a host attaches, which no provider fills.
HOST_RESPONSE_FIELDS = {"tasks", "observations"}

# The instant every case runs at, in seconds since 1970: 2026-01-01T00:00:00Z.
NOW = 1767225600.0

NOTES = {
    "config": "The keyword arguments of OpenAIConfig. The key is a placeholder.",
    "request": (
        "AIContext dumped with model_dump(mode='json'), its ten request fields. A key the "
        "request carries is written masked there, and plain under request_api_key."
    ),
    "exchanges.request": (
        "What Python sent: the method, the path with its query, the body as JSON, and the "
        "headers RoomKit decides: authorization, with the key replaced by which key it was "
        "(<config key> or <request key>), content-type, accept, and each configured default "
        "header. The headers of the SDK and of the platform are left out. timeouts holds, in "
        "seconds, how long the request may wait to connect, for a read, for a write and for "
        "a connection of the pool."
    ),
    "exchanges.response": (
        "What the fake answered: status and headers with a body under json, under text for a "
        "body that is no JSON, or under chunks for a stream, each chunk the UTF-8 text of one "
        "read, in order; then says whether the stream ended or the connection was lost after "
        "them. A failure in place of a response is under failure: no answer came."
    ),
    "exchanges.wait": (
        "The wait the openai SDK took before its next try, in seconds, and where it came "
        "from: the answer's headers, or the SDK's backoff, which is up to a quarter less than "
        "what is written. An HTTP date is read against clock."
    ),
    "expected": (
        "What Python returned: response for generate (AIResponse without tasks and "
        "observations), text for generate_stream (the deltas), events for "
        "generate_structured_stream (each event dumped). A failure is under error, after "
        "whatever a stream gave first: its class, its message (the first argument), its text "
        "(str), the attributes of a ProviderError, and raised_from, the class of the SDK or "
        "transport error it was raised from, if any."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call its server gave none: "
        "call_<name>_<12 hexadecimal digits>, random. A replay compares their shape, and that "
        "the events of one call share one id, not their value."
    ),
    "sdk": (
        "Where a case says sdk, the openai SDK decided the behaviour, not RoomKit: its "
        "retries (which answers, how many times, after what wait) and the words of a "
        "failure that has no answer."
    ),
}


class Clock:
    """The ``time`` module, its clock stopped at ``NOW``."""

    def time(self) -> float:
        return NOW

    def __getattr__(self, name: str) -> Any:
        return getattr(time, name)


class MintedIds:
    """``uuid4`` for a call's minted id, counted instead of drawn."""

    def __init__(self) -> None:
        self.drawn: list[str] = []

    def __call__(self) -> Any:
        self.drawn.append(f"{len(self.drawn) + 1:012x}")
        return type("Drawn", (), {"hex": self.drawn[-1] + "0" * 20})()


class Chunks(httpx2.AsyncByteStream):
    """A streamed body sent chunk by chunk, the connection lost after them
    when ``cut``."""

    def __init__(self, chunks: list[str], cut: bool) -> None:
        self._chunks = chunks
        self._cut = cut

    async def __aiter__(self) -> AsyncIterator[bytes]:
        for chunk in self._chunks:
            yield chunk.encode("utf-8")
        if self._cut:
            raise httpx2.RemoteProtocolError(
                "peer closed connection without sending complete message body"
            )


def answered(reply: Reply) -> dict[str, Any]:
    """``reply`` as the fixture writes it."""
    if reply.failure is not None:
        return {"failure": reply.failure}
    written: dict[str, Any] = {"status": reply.status, "headers": reply.headers}
    if reply.chunks is not None:
        ending = "the connection is lost" if reply.cut else "the stream ends"
        return {**written, "chunks": reply.chunks, "then": ending}
    if reply.text is not None:
        return {**written, "text": reply.text}
    return {**written, "json": reply.body}


def response_of(reply: Reply, request: httpx2.Request) -> httpx2.Response:
    """The response the fake gives for ``reply``, or the failure it raises."""
    if reply.failure == "connection refused":
        raise httpx2.ConnectError("All connection attempts failed", request=request)
    if reply.failure == "timeout":
        raise httpx2.ReadTimeout("The read operation timed out", request=request)
    if reply.chunks is not None:
        body = Chunks(reply.chunks, reply.cut)
        return httpx2.Response(reply.status, headers=reply.headers, stream=body)
    if reply.text is not None:
        return httpx2.Response(reply.status, headers=reply.headers, text=reply.text)
    content = json.dumps(reply.body).encode("utf-8")
    return httpx2.Response(reply.status, headers=reply.headers, content=content)


def authorization(value: str | None) -> str | None:
    """An ``Authorization`` header with its key replaced by which key it was."""
    if value is None:
        return None
    named = value.replace(REQUEST_KEY, "<request key>").replace(KEY, "<config key>")
    if "placeholder-not-a-key" in named:
        raise AssertionError("an Authorization header holds a key the fixture cannot name")
    return named


def sent(request: httpx2.Request, case: Case) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {
        "authorization": authorization(request.headers.get("authorization")),
        "content-type": request.headers.get("content-type"),
        "accept": request.headers.get("accept"),
    }
    for name in case.config.get("default_headers") or {}:
        if name.lower() not in headers:
            headers[name.lower()] = request.headers.get(name)
    return {
        "method": request.method,
        "path": request.url.raw_path.decode("ascii"),
        "headers": headers,
        "timeouts": request.extensions["timeout"],
        "body": json.loads(request.content) if request.content else None,
    }


@dataclass
class Wire:
    """The fake endpoint of one case: its replies in order, and what was
    exchanged."""

    case: Case
    exchanges: list[dict[str, Any]] = field(default_factory=list)

    async def __call__(self, request: httpx2.Request) -> httpx2.Response:
        asked = len(self.exchanges)
        if asked >= len(self.case.replies):
            raise AssertionError(f"{self.case.name}: a request more than the case answers")
        reply = self.case.replies[asked]
        self.exchanges.append({"request": sent(request, self.case), "response": answered(reply)})
        return response_of(reply, request)

    def record_waits(self, client: Any) -> None:
        """Record the wait the SDK computes before each next try, and take
        none: its longest, and whether the answer's headers decided it."""
        compute = client._calculate_retry_timeout

        def recorded(*args: Any, **kwargs: Any) -> float:
            sdk.random = lambda: 0.0
            longest = compute(*args, **kwargs)
            sdk.random = lambda: 1.0
            decided = longest == compute(*args, **kwargs)
            source = "the answer's headers" if decided else "the SDK's backoff"
            self.exchanges[-1]["wait"] = {"seconds": longest, "from": source}
            return 0.0

        client._calculate_retry_timeout = recorded


def error_form(failed: BaseException) -> dict[str, Any]:
    """An error as the fixture writes it."""
    form: dict[str, Any] = {
        "class": type(failed).__name__,
        "message": str(failed.args[0]) if failed.args else "",
        "text": str(failed),
    }
    if isinstance(failed, ProviderError):
        form.update({name: value for name, value in sorted(vars(failed).items())})
    cause = failed.__cause__
    form["raised_from"] = type(cause).__name__ if cause is not None else None
    json.dumps(form)
    return form


async def outcome(provider: OpenAIAIProvider, call: str, context: AIContext) -> dict[str, Any]:
    """What ``call`` returns for ``context``, or what it gave before it
    failed and the error it raised."""
    given: list[Any] = []
    try:
        if call == "generate":
            response = await provider.generate(context)
            return {"response": response.model_dump(mode="json", exclude=HOST_RESPONSE_FIELDS)}
        if call == "generate_stream":
            async for text in provider.generate_stream(context):
                given.append(text)
            return {"text": given}
        async for event in provider.generate_structured_stream(context):
            given.append(event.model_dump(mode="json"))
        return {"events": given}
    except Exception as failed:
        if call == "generate":
            return {"error": error_form(failed)}
        key = "text" if call == "generate_stream" else "events"
        return {key: given, "error": error_form(failed)}


async def answered_by_python(case: Case, wire: Wire, context: AIContext) -> dict[str, Any]:
    """What the provider of ``case`` returns over ``wire``, or the error that
    kept it from being built."""
    transport = httpx2.MockTransport(wire)
    try:
        provider = OpenAIAIProvider(OpenAIConfig(**case.config), transport=transport)
    except openai.OpenAIError as unbuilt:
        return {"error": error_form(unbuilt)}
    wire.record_waits(provider._client)
    try:
        return await outcome(provider, case.call, context)
    finally:
        await provider.close()


def ids_in(value: Any) -> list[str]:
    """Every ``id`` of ``value``, in order."""
    if isinstance(value, dict):
        own = [value["id"]] if isinstance(value.get("id"), str) else []
        return own + [found for item in value.values() for found in ids_in(item)]
    if isinstance(value, list):
        return [found for item in value for found in ids_in(item)]
    return []


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    minted = tool_calls.uuid4 = MintedIds()
    wire = Wire(case)
    context = AIContext.model_validate(case.request)
    expected = await answered_by_python(case, wire, context)
    if len(wire.exchanges) != len(case.replies):
        raise AssertionError(f"{case.name}: {len(case.replies)} replies, not all asked for")
    written: dict[str, Any] = {"name": case.name, "kinds": sorted(case.kinds), "call": case.call}
    if case.note:
        written["note"] = case.note
    if case.sdk:
        written["sdk"] = case.sdk
    written["config"] = case.config
    written["request"] = context.model_dump(mode="json", include=REQUEST_FIELDS)
    if "api_key" in case.request.get("metadata", {}):
        written["request_api_key"] = case.request["metadata"]["api_key"]
    written["exchanges"] = wire.exchanges
    written["expected"] = expected
    drawn = [found for found in ids_in(expected) if found.rpartition("_")[2] in minted.drawn]
    if drawn:
        written["minted_call_ids"] = list(dict.fromkeys(drawn))
    return written


def main() -> None:
    # A warning the provider logs once is no part of what a case returns.
    logging.getLogger("roomkit").setLevel(logging.ERROR)
    # The SDK reads its key, its base URL and more from the environment when
    # a client is given none: a case is what its configuration says.
    for name in [name for name in os.environ if name.startswith("OPENAI_")]:
        del os.environ[name]
    sdk.time = Clock()
    names = [case.name for case in CASES]
    if len(set(names)) != len(names):
        raise AssertionError("two cases share a name")
    cases = [asyncio.run(run(case)) for case in CASES]
    fixture = {
        "generator": "scripts/generate_openai_chat_cases.py",
        "roomkit_version": roomkit.__version__,
        "openai_version": openai.__version__,
        "httpx2_version": httpx2.__version__,
        "python_version": platform.python_version(),
        "clock": "2026-01-01T00:00:00Z",
        "notes": NOTES,
        "cases": cases,
    }
    text = json.dumps(fixture, indent=4)
    if REQUEST_KEY in text.replace(f'"request_api_key": "{REQUEST_KEY}"', ""):
        raise AssertionError("a request's key is written outside request_api_key")
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(text + "\n", encoding="utf-8")
    failed = sum("error" in case["expected"] for case in cases)
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(cases)} cases, {failed} that fail, "
        f"from roomkit {roomkit.__version__}, openai {openai.__version__}, "
        f"httpx2 {httpx2.__version__}, Python {platform.python_version()}"
    )


if __name__ == "__main__":
    main()
