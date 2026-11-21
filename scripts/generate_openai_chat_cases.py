#!/usr/bin/env python3
"""Write what RoomKit's Python OpenAI provider sends and reads, case by case.

Each case runs ``OpenAIAIProvider`` once, on Chat Completions or on the model
listing, against a fake of the wire (``httpx2.MockTransport``: nothing leaves
this script), and records:

- ``config``: the keyword arguments of its ``OpenAIConfig``;
- ``request``: the contract's JSON form of the request (the ten fields of
  ``AIContext`` a provider reads), and ``request_api_key`` when the request
  carries a key of its own, which the form writes masked;
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it;
- ``expected``: what Python returned, in the contract's JSON forms, or the
  error it raised, and ``compare``, how a replay compares that error.

The Rust provider replays the table: the same requests sent, the same result
read. ``NOTES`` says how each part is written and how a replay compares what
no two runs give alike.

Run it with a Python interpreter that has ``roomkit`` and its ``openai``
extra installed (``make python-openai-chat-cases``). The fixture records the
versions and the commit of ``roomkit`` it was written from.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import platform
import subprocess
import time
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import httpx2
import openai
import pydantic
import roomkit
from openai import _base_client as sdk
from openai_chat_cases import CASES, KEY, LOST, REQUEST_KEY, Case, Reply
from roomkit.providers.ai import AIContext, ProviderError, tool_calls
from roomkit.providers.ai.reasoning import reasoning_floor
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

# The tags of a model's catalogue entry the provider reads on Chat Completions.
READ_TAGS = (
    "responses_only",
    "chat_tools_refused",
    "tools_reasoning_none",
    "tools_reasoning_effort",
)

# The attributes of a ``ProviderError`` a replay reproduces exactly.
EXACT = ["class", "status_code", "retryable", "provider", "context_overflow", "before_request"]

# The classes the openai SDK raises for an answer whose status is an error.
STATUS_ERRORS = {
    "APIStatusError",
    "BadRequestError",
    "AuthenticationError",
    "PermissionDeniedError",
    "NotFoundError",
    "ConflictError",
    "UnprocessableEntityError",
    "RateLimitError",
    "InternalServerError",
}

# The classes of the openai SDK a call may raise as they are.
SDK_ERRORS = {"OpenAIError", "APIConnectionError", "APITimeoutError", *STATUS_ERRORS}

# The classes the HTTP client raises for a body it could not read to its end.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    "environment": (
        "Every OPENAI_* variable of the environment is removed before a case runs: the "
        "openai SDK reads its key, its base URL and more from them when a client is given "
        "none. No case is captured with one set."
    ),
    "versions": (
        "The versions the fixture was written from. A message in Python's own words, which "
        "no replay compares, may name one: pydantic's names its version in a link."
    ),
    "provenance": (
        "roomkit_commit is the commit roomkit was imported at, and roomkit_dirty whether "
        "the package's files had changes not committed then. Both are null unless the "
        "package's own files are tracked by the repository that holds them: a package "
        "installed inside some checkout is not that checkout's. catalogue, on a case that "
        "calls OpenAI's own "
        "endpoint, holds what the provider read of its model's catalogue entry: whether the "
        "catalogue knows the model, the tags among those it reads on Chat Completions, and "
        "the least reasoning effort the entry declares. A case that changes because the "
        "catalogue changed shows it there."
    ),
    "config": "The keyword arguments of OpenAIConfig. The key is a placeholder.",
    "supports_vision": (
        "What the provider of the case answers for supports_vision: whether its model takes "
        "images. It is left out where no provider was built."
    ),
    "request": (
        "AIContext dumped with model_dump(mode='json'), its ten request fields. A key the "
        "request carries is written masked there, and plain under request_api_key."
    ),
    "exchanges.request": (
        "What Python sent: the method, the whole URL (scheme, host, a port that is written, "
        "path and query), the body as JSON, and the headers RoomKit decides: authorization, "
        "with the key replaced by which key it was (<config key> or <request key>), "
        "content-type, accept, and each configured default header. The headers of the SDK "
        "and of the platform are left out. timeouts holds, in seconds, how long the request "
        "may wait to connect, for a read, for a write and for a connection of the pool."
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
        "The wait the openai SDK took before its next try, in seconds, and where it came "
        "from: the answer's headers, or the SDK's backoff, which is up to a quarter less than "
        "what is written. An HTTP date is read against clock."
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
        "On a case that fails, how a replay compares the failure. exactly lists what it "
        "reproduces as written: attributes of the error, reason among them where the error "
        "is a response schema's, and events or text, what the stream gave before it "
        "failed. An empty list is a failure no provider reproduces: the "
        "SDK's own, or a raw Python exception. message says whose words the message holds, "
        "and so whether it is compared: roomkit (RoomKit's own words: compared), server (the "
        "text the server sent, handed on as it came: compared), sdk (the openai SDK's words: "
        "not compared), python_json (what Python's JSON reader says of a text: not "
        "compared), python_repr (Python's repr of the body the server sent, after 'Error "
        "code: <status> - ': not compared), transport (the HTTP client's words for a body "
        "it could not read to its end: not compared), python (the words of a Python "
        "exception raised while the answer was read: not compared). A python_json message "
        "leaves RoomKit's own words before it uncompared too ('the answer is not a JSON "
        "document (...)'). A compared roomkit message may hold Python's repr of a name or "
        "of a list (tool 'orders.lookup', missing ['city']), which a replay writes the same "
        "way."
    ),
    "minted_call_ids": (
        "The ids in expected that Python minted for a call its server gave none: "
        "call_<name>_<12 hexadecimal digits>, random. A replay compares their shape, and that "
        "the events of one call share one id, not their value."
    ),
    "sdk": (
        "Where a case says sdk, the openai SDK decided the behaviour, not RoomKit: its "
        "retries (which answers, how many times, after what wait), how it joins a URL, "
        "merges a body or follows a redirect, how it reads a body, and the words of a "
        "failure that has no answer. A provider without that SDK decides these itself."
    ),
    "questioned": (
        "Where a case says questioned, it pins a behaviour of Python that is not settled, "
        "in the sentence given, which starts with its kind. 'A possible defect': reported "
        "upstream as one. 'A difference to decide': a point where the Rust provider is "
        "planned otherwise, which the maintainer decides; no defect is claimed. A replay may "
        "set the case aside, or expect otherwise once it is settled. The fixture lists "
        "their names under questioned."
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


def read_bytes(chunk: str | bytes) -> bytes:
    return chunk if isinstance(chunk, bytes) else chunk.encode("utf-8")


class Chunks(httpx2.AsyncByteStream):
    """A body sent read by read; its reads then end, or fail as ``cut`` says."""

    def __init__(self, chunks: list[str] | list[bytes], cut: str | None) -> None:
        self._chunks = chunks
        self._cut = cut

    async def __aiter__(self) -> AsyncIterator[bytes]:
        for chunk in self._chunks:
            yield read_bytes(chunk)
        if self._cut == LOST:
            raise httpx2.RemoteProtocolError(
                "peer closed connection without sending complete message body"
            )
        if self._cut is not None:
            raise httpx2.ReadTimeout("The read operation timed out")


def written_chunk(chunk: str | bytes) -> str | dict[str, str]:
    """The bytes of one read: as their text where they are UTF-8, else in
    hexadecimal."""
    data = read_bytes(chunk)
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return {"hex": data.hex()}


def answered(reply: Reply) -> dict[str, Any]:
    """``reply`` as the fixture writes it."""
    if reply.failure is not None:
        return {"failure": reply.failure}
    written: dict[str, Any] = {"status": reply.status, "headers": reply.headers}
    if reply.chunks is not None:
        chunks = [written_chunk(chunk) for chunk in reply.chunks]
        return {**written, "chunks": chunks, "then": reply.cut or "the stream ends"}
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
        "url": str(request.url),
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


def texts_in(value: Any) -> list[str]:
    """Every text ``value`` holds, itself included."""
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [found for item in value.values() for found in texts_in(item)]
    if isinstance(value, list):
        return [found for item in value for found in texts_in(item)]
    return []


def answered_texts(exchanges: list[dict[str, Any]]) -> list[str]:
    """Every text the fake sent: a text body stripped, each text of a JSON
    body, and each text of the JSON a stream's ``data`` lines hold."""
    texts: list[str] = []
    for exchange in exchanges:
        answer = exchange["response"]
        texts += [answer.get("text", "").strip(), *texts_in(answer.get("json"))]
        sent = b"".join(read_bytes(chunk) for chunk in answer.get("sent", []))
        for line in sent.decode("utf-8", errors="replace").splitlines():
            try:
                texts += texts_in(json.loads(line.removeprefix("data:")))
            except ValueError:
                continue
    return [text for text in texts if text]


def words_of(error: dict[str, Any], sent: list[str]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``);
    ``sent`` is every text the fake answered with."""
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
    if message in sent:
        return "server"
    if cause == "APIError":
        # An error event that holds no message: the SDK's own words.
        return "sdk"
    return "roomkit" if cause in (None, "ValueError") else "python"


def comparison(case: Case, wire: Wire, expected: dict[str, Any]) -> dict[str, Any] | None:
    """How a replay compares the failure of ``case``, when it failed."""
    error = expected.get("error")
    if error is None:
        return None
    sent = [
        {"response": {**answered(reply), "sent": reply.chunks or []}} for reply in case.replies
    ]
    words = words_of(error, answered_texts(sent[: len(wire.exchanges)]))
    if "provider" not in error:
        return {"exactly": [], "message": words}
    reason = ["reason"] if "reason" in error else []
    given = [key for key in ("events", "text") if key in expected]
    return {"exactly": [*EXACT, *reason, *given], "message": words}


async def outcome(provider: OpenAIAIProvider, call: str, context: AIContext) -> dict[str, Any]:
    """What ``call`` returns for ``context``, or what it gave before it
    failed and the error it raised."""
    given: list[Any] = []
    try:
        if call == "generate":
            response = await provider.generate(context)
            return {"response": response.model_dump(mode="json", exclude=HOST_RESPONSE_FIELDS)}
        if call == "list_models":
            models = await provider.list_models()
            return {"models": [model.model_dump(mode="json") for model in models]}
        if call == "generate_stream":
            async for text in provider.generate_stream(context):
                given.append(text)
            return {"text": given}
        async for event in provider.generate_structured_stream(context):
            given.append(event.model_dump(mode="json"))
        return {"events": given}
    except Exception as failed:
        if call in ("generate", "list_models"):
            return {"error": error_form(failed)}
        key = "text" if call == "generate_stream" else "events"
        return {key: given, "error": error_form(failed)}


def catalogue_of(provider: OpenAIAIProvider) -> dict[str, Any] | None:
    """What ``provider`` reads of its model's catalogue entry; ``None``
    behind a base URL, where it reads none."""
    if not provider._is_openai_endpoint:
        return None
    entry = provider.catalog_entry()
    tags = [tag for tag in READ_TAGS if entry is not None and tag in entry.capabilities]
    return {
        "model": provider.model_name,
        "known": entry is not None,
        "tags": tags,
        "reasoning_floor": reasoning_floor(entry),
    }


async def answered_by_python(case: Case, wire: Wire, context: AIContext) -> dict[str, Any]:
    """What the provider of ``case`` returns over ``wire`` and what it read
    of the catalogue, or the error that kept it from being built."""
    transport = httpx2.MockTransport(wire)
    try:
        provider = OpenAIAIProvider(OpenAIConfig(**case.config), transport=transport)
    except openai.OpenAIError as unbuilt:
        return {"expected": {"error": error_form(unbuilt)}, "catalogue": None, "vision": None}
    wire.record_waits(provider._client)
    try:
        expected = await outcome(provider, case.call, context)
        vision = provider.supports_vision
        return {"expected": expected, "catalogue": catalogue_of(provider), "vision": vision}
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
    ran = await answered_by_python(case, wire, context)
    expected = ran["expected"]
    if len(wire.exchanges) != len(case.replies):
        raise AssertionError(f"{case.name}: {len(case.replies)} replies, not all asked for")
    written: dict[str, Any] = {"name": case.name, "kinds": sorted(case.kinds), "call": case.call}
    for member in ("note", "sdk", "questioned"):
        if getattr(case, member):
            written[member] = getattr(case, member)
    written["config"] = case.config
    if ran["catalogue"] is not None:
        written["catalogue"] = ran["catalogue"]
    if ran["vision"] is not None:
        written["supports_vision"] = ran["vision"]
    written["request"] = context.model_dump(mode="json", include=REQUEST_FIELDS)
    if "api_key" in case.request.get("metadata", {}):
        written["request_api_key"] = case.request["metadata"]["api_key"]
    written["exchanges"] = wire.exchanges
    written["expected"] = expected
    compare = comparison(case, wire, expected)
    if compare is not None:
        written["compare"] = compare
    drawn = [found for found in ids_in(expected) if found.rpartition("_")[2] in minted.drawn]
    if drawn:
        written["minted_call_ids"] = list(dict.fromkeys(drawn))
    return written


def git(package: Path, *arguments: str) -> str | None:
    """What ``git`` answers in ``package``, ``None`` when it fails."""
    command = ["git", "-C", str(package), *arguments]
    found = subprocess.run(command, capture_output=True, text=True, check=False)
    return found.stdout if found.returncode == 0 else None


def roomkit_source() -> tuple[str | None, bool | None]:
    """The commit ``roomkit`` was imported at, and whether its files had
    changes not committed: both ``None`` unless the package's own files are
    tracked by the repository that holds them. A package installed in a
    virtual environment inside some checkout is not that checkout's."""
    package = Path(roomkit.__file__).resolve().parent
    if git(package, "ls-files", "--error-unmatch", "__init__.py") is None:
        return None, None
    commit, changes = git(package, "rev-parse", "HEAD"), git(package, "status", "--porcelain", ".")
    if commit is None or changes is None:
        return None, None
    return commit.strip(), bool(changes.strip())


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
    commit, dirty = roomkit_source()
    fixture = {
        "generator": "scripts/generate_openai_chat_cases.py",
        "roomkit_version": roomkit.__version__,
        "roomkit_commit": commit,
        "roomkit_dirty": dirty,
        "openai_version": openai.__version__,
        "pydantic_version": pydantic.__version__,
        "httpx2_version": httpx2.__version__,
        "python_version": platform.python_version(),
        "clock": "2026-01-01T00:00:00Z",
        "notes": NOTES,
        "questioned": [case["name"] for case in cases if "questioned" in case],
        "cases": cases,
    }
    # A number JSON does not name must not reach the fixture.
    text = json.dumps(fixture, indent=1, allow_nan=False)
    if REQUEST_KEY in text.replace(f'"request_api_key": "{REQUEST_KEY}"', ""):
        raise AssertionError("a request's key is written outside request_api_key")
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(text + "\n", encoding="utf-8")
    failed = sum("error" in case["expected"] for case in cases)
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(cases)} cases, {failed} that fail, "
        f"{len(fixture['questioned'])} questioned, from roomkit {roomkit.__version__}, "
        f"openai {openai.__version__}, httpx2 {httpx2.__version__}, "
        f"Python {platform.python_version()}"
    )


if __name__ == "__main__":
    main()
