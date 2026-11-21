"""What the generators of a provider's wire cases share.

A generator runs RoomKit's Python provider case by case against a fake of the
wire and writes what was exchanged and what the provider returned. This
module holds what does not depend on the provider: what a case is, how the
fake answers, how an exchange, an outcome and a failure are written, and how
the fixture is put together.

``generate_openai_chat_cases.py`` and ``generate_anthropic_messages_cases.py``
each add what their provider and its SDK decide: where the fake sits, which
headers a request is written with, and whose words a failure's message holds.
"""

from __future__ import annotations

import json
import logging
import os
import platform
import subprocess
import time
from collections.abc import AsyncIterator, Callable, Iterable
from dataclasses import dataclass, field, replace
from pathlib import Path
from types import ModuleType
from typing import Any

import httpx2
import pydantic
import roomkit
from roomkit.providers.ai import AIContext, AIProvider, ProviderError

KEY = "placeholder-not-a-key"
REQUEST_KEY = "placeholder-not-a-key-either"

JSON = {"content-type": "application/json"}
EVENTS = {"content-type": "text/event-stream"}

# The calls of a provider a case may run.
GENERATE = "generate"
TEXT = "generate_stream"
STREAM = "generate_structured_stream"
LISTING = "list_models"

# How the reads of a body fail after its chunks.
LOST = "the connection is lost"
TIMEOUT = "the read times out"

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
CLOCK = "2026-01-01T00:00:00Z"

# The attributes of a ``ProviderError`` a replay reproduces exactly.
EXACT = ["class", "status_code", "retryable", "provider", "context_overflow", "before_request"]


# -- What a case is ---------------------------------------------------------------


@dataclass(frozen=True)
class Reply:
    """What the fake endpoint answers one request with."""

    status: int = 200
    headers: dict[str, str] = field(default_factory=dict)
    body: Any = None
    text: str | None = None
    chunks: list[str] | list[bytes] | None = None
    cut: str | None = None
    failure: str | None = None


@dataclass(frozen=True)
class Turn:
    """A call of a case's provider made before the case's own: its request,
    and what the fake answers it."""

    request: dict[str, Any]
    replies: tuple[Reply, ...]


@dataclass(frozen=True)
class Case:
    """One run of the provider."""

    name: str
    kinds: frozenset[str]
    call: str
    config: dict[str, Any]
    request: dict[str, Any]
    replies: tuple[Reply, ...]
    note: str | None = None
    sdk: str | None = None
    questioned: str | None = None
    before: tuple[Turn, ...] = ()


REFUSED = Reply(failure="connection refused")
TIMED_OUT = Reply(failure="timeout")


def registrar(cases: list[Case]) -> Callable[..., None]:
    """The function a table of cases adds each of its cases with."""

    def case(
        name: str,
        kinds: str,
        call: str,
        config: dict[str, Any],
        request: dict[str, Any],
        *replies: Reply,
        note: str | None = None,
        sdk: str | None = None,
        before: tuple[Turn, ...] = (),
    ) -> None:
        kinds_named = frozenset(kinds.split())
        cases.append(
            Case(name, kinds_named, call, config, request, replies, note, sdk, before=before)
        )

    return case


def annotate(cases: list[Case], member: str, sentences: dict[str, str]) -> None:
    """Write ``sentences``, each under the name of its case, into ``member``
    of the cases."""
    by_name = {found.name: at for at, found in enumerate(cases)}
    for name, sentence in sentences.items():
        found = cases[by_name[name]]
        if getattr(found, member) is not None:
            raise AssertionError(f"{name} already says {member}")
        cases[by_name[name]] = replace(found, **{member: sentence})


# -- What the fake answers -----------------------------------------------------------


def answer(body: Any, status: int = 200, **headers: str) -> Reply:
    return Reply(status=status, headers={**JSON, **headers}, body=body)


def page(text: str, status: int, content_type: str = "text/html") -> Reply:
    return Reply(status=status, headers={"content-type": content_type}, text=text)


def stream(chunks: list[str] | list[bytes], *, cut: str | None = None, status: int = 200) -> Reply:
    """A streamed answer, read chunk by chunk; ``cut`` is how its reads fail
    after them, ``LOST`` or ``TIMEOUT``."""
    return Reply(status=status, headers=dict(EVENTS), chunks=chunks, cut=cut)


def half(body: Any, cut: str) -> Reply:
    """A whole answer whose body fails midway, ``LOST`` or ``TIMEOUT``."""
    text = json.dumps(body)
    return Reply(status=200, headers=dict(JSON), chunks=[text[: len(text) // 2]], cut=cut)


def cut_every(chunks: list[str], size: int) -> list[str]:
    """The same bytes, read ``size`` characters at a time."""
    whole = "".join(chunks)
    return [whole[at : at + size] for at in range(0, len(whole), size)]


def cut_inside_characters(chunks: list[str]) -> list[bytes]:
    """The same bytes, each character of several bytes read one byte at a
    time: a read ends before it, and after each of its bytes."""
    text = "".join(chunks)
    data = text.encode("utf-8")
    edges, at = {0, len(data)}, 0
    for character in text:
        size = len(character.encode("utf-8"))
        if size > 1:
            edges.update(range(at, at + size + 1))
        at += size
    ordered = sorted(edges)
    return [data[start:end] for start, end in zip(ordered, ordered[1:], strict=False)]


def one_byte_at_a_time(chunks: list[str]) -> list[bytes]:
    data = "".join(chunks).encode("utf-8")
    return [data[at : at + 1] for at in range(len(data))]


# -- The fake of the wire -------------------------------------------------------------


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


def named_key(value: str | None, request_key: str | None = REQUEST_KEY) -> str | None:
    """A header's value with its key replaced by which key it was: the
    configuration's, or the one the request carries."""
    if value is None:
        return None
    named = value
    if request_key is not None:
        named = named.replace(request_key, "<request key>")
    named = named.replace(KEY, "<config key>")
    # Every placeholder starts as the configuration's does: one that is
    # neither of the two would be written as the configuration's and more.
    if KEY in named or "<config key>-" in named:
        raise AssertionError("a header holds a key the fixture cannot name")
    return named


def request_form(request: httpx2.Request, headers: dict[str, Any]) -> dict[str, Any]:
    """``request`` as the fixture writes it, with the ``headers`` its
    generator keeps of it."""
    return {
        "method": request.method,
        "url": str(request.url),
        "headers": headers,
        "timeouts": request.extensions["timeout"],
        "body": json.loads(request.content) if request.content else None,
    }


@dataclass
class Wire:
    """The fake endpoint of one call: its replies in order, and what was
    exchanged. ``sent`` writes a request as the fixture holds it."""

    name: str
    replies: tuple[Reply, ...]
    sent: Callable[[httpx2.Request], dict[str, Any]]
    exchanges: list[dict[str, Any]] = field(default_factory=list)

    async def __call__(self, request: httpx2.Request) -> httpx2.Response:
        asked = len(self.exchanges)
        if asked >= len(self.replies):
            raise AssertionError(f"{self.name}: a request more than the case answers")
        reply = self.replies[asked]
        self.exchanges.append({"request": self.sent(request), "response": answered(reply)})
        return response_of(reply, request)

    def record_waits(self, clients: Any, sdk: ModuleType) -> None:
        """Record the wait the SDK computes before each next try, and take
        none: its longest, and whether the answer's headers decided it.
        ``clients`` is one client of the SDK, or the class of all of them;
        ``sdk`` is the module of the SDK that draws the wait."""
        compute = clients._calculate_retry_timeout

        def recorded(*args: Any, **kwargs: Any) -> float:
            sdk.random = lambda: 0.0
            longest = compute(*args, **kwargs)
            sdk.random = lambda: 1.0
            decided = longest == compute(*args, **kwargs)
            source = "the answer's headers" if decided else "the SDK's backoff"
            self.exchanges[-1]["wait"] = {"seconds": longest, "from": source}
            return 0.0

        clients._calculate_retry_timeout = recorded

    def all_asked_for(self) -> None:
        if len(self.exchanges) != len(self.replies):
            raise AssertionError(f"{self.name}: {len(self.replies)} replies, not all asked for")


# -- What a call gives ----------------------------------------------------------------


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


async def outcome(provider: AIProvider, call: str, context: AIContext) -> dict[str, Any]:
    """What ``call`` returns for ``context``, or what it gave before it
    failed and the error it raised."""
    given: list[Any] = []
    try:
        if call == GENERATE:
            response = await provider.generate(context)
            return {"response": response.model_dump(mode="json", exclude=HOST_RESPONSE_FIELDS)}
        if call == LISTING:
            models = await provider.list_models()
            return {"models": [model.model_dump(mode="json") for model in models]}
        if call == TEXT:
            async for text in provider.generate_stream(context):
                given.append(text)
            return {"text": given}
        async for event in provider.generate_structured_stream(context):
            given.append(event.model_dump(mode="json"))
        return {"events": given}
    except Exception as failed:
        if call in (GENERATE, LISTING):
            return {"error": error_form(failed)}
        key = "text" if call == TEXT else "events"
        return {key: given, "error": error_form(failed)}


def texts_in(value: Any) -> list[str]:
    """Every text ``value`` holds, itself included."""
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [found for item in value.values() for found in texts_in(item)]
    if isinstance(value, list):
        return [found for item in value for found in texts_in(item)]
    return []


def data_lines(replies: Iterable[Reply]) -> list[str]:
    """What each ``data`` line of the streams among ``replies`` holds."""
    held: list[str] = []
    for reply in replies:
        sent = b"".join(read_bytes(chunk) for chunk in reply.chunks or [])
        for line in sent.decode("utf-8", errors="replace").splitlines():
            held.append(line.removeprefix("data:"))
    return held


def answered_texts(replies: Iterable[Reply]) -> list[str]:
    """Every text the fake sent with ``replies``: a text body stripped, each
    text of a JSON body, and each text of the JSON a stream's ``data`` lines
    hold."""
    texts: list[str] = []
    for reply in replies:
        if reply.failure is not None:
            continue
        if reply.chunks is not None:
            texts += [text for line in data_lines([reply]) for text in texts_of_json(line)]
        elif reply.text is not None:
            texts.append(reply.text.strip())
        else:
            texts += texts_in(reply.body)
    return [text for text in texts if text]


def texts_of_json(line: str) -> list[str]:
    """Every text of the JSON ``line`` is, none when it is no JSON."""
    try:
        return texts_in(json.loads(line))
    except ValueError:
        return []


def comparison(
    expected: dict[str, Any], words: Callable[[dict[str, Any]], str]
) -> dict[str, Any] | None:
    """How a replay compares the failure of ``expected``, when it failed:
    ``words`` says whose words the error's message holds."""
    error = expected.get("error")
    if error is None:
        return None
    if "provider" not in error:
        return {"exactly": [], "message": words(error)}
    reason = ["reason"] if "reason" in error else []
    given = [key for key in ("events", "text") if key in expected]
    return {"exactly": [*EXACT, *reason, *given], "message": words(error)}


def ids_in(value: Any) -> list[str]:
    """Every ``id`` of ``value``, in order."""
    if isinstance(value, dict):
        own = [value["id"]] if isinstance(value.get("id"), str) else []
        return own + [found for item in value.values() for found in ids_in(item)]
    if isinstance(value, list):
        return [found for item in value for found in ids_in(item)]
    return []


# -- How a case is written --------------------------------------------------------------


def turn_form(
    given: dict[str, Any],
    context: AIContext,
    wire: Wire,
    expected: dict[str, Any],
    words: Callable[[dict[str, Any]], str],
    minted: MintedIds,
) -> dict[str, Any]:
    """One call of a case as the fixture writes it: the request ``given``
    as ``context`` dumps it, what was exchanged, and what the call gave."""
    wire.all_asked_for()
    written: dict[str, Any] = {"request": context.model_dump(mode="json", include=REQUEST_FIELDS)}
    if "api_key" in given.get("metadata", {}):
        written["request_api_key"] = given["metadata"]["api_key"]
    written["exchanges"] = wire.exchanges
    written["expected"] = expected
    compare = comparison(expected, words)
    if compare is not None:
        written["compare"] = compare
    drawn = [found for found in ids_in(expected) if found.rpartition("_")[2] in minted.drawn]
    if drawn:
        written["minted_call_ids"] = list(dict.fromkeys(drawn))
    return written


def case_form(case: Case, told: dict[str, Any], turns: list[dict[str, Any]]) -> dict[str, Any]:
    """One case as the fixture writes it. ``told`` is what the provider says
    of itself, a member it leaves ``None`` left out; ``turns`` are the calls
    made before the case's own, then its own."""
    written: dict[str, Any] = {"name": case.name, "kinds": sorted(case.kinds), "call": case.call}
    for member in ("note", "sdk", "questioned"):
        if getattr(case, member):
            written[member] = getattr(case, member)
    written["config"] = case.config
    written.update({name: value for name, value in told.items() if value is not None})
    if turns[:-1]:
        written["before"] = turns[:-1]
    return {**written, **turns[-1]}


# -- How the fixture is put together ------------------------------------------------------


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


def prepare(sdk: ModuleType, variables: str) -> None:
    """Make a run say the same whatever the machine: the provider's warnings
    left out, the environment variables an SDK reads in place of what its
    client is given removed (those that start with ``variables``), and the
    clock of ``sdk``, the SDK's module that reads it, stopped."""
    # A warning the provider logs once is no part of what a case returns.
    logging.getLogger("roomkit").setLevel(logging.ERROR)
    for name in [name for name in os.environ if name.startswith(variables)]:
        del os.environ[name]
    sdk.time = Clock()


def unique_names(cases: list[Case]) -> None:
    names = [case.name for case in cases]
    if len(set(names)) != len(names):
        raise AssertionError("two cases share a name")


def fixture_of(
    generator: str, sdk: ModuleType, notes: dict[str, str], cases: list[dict[str, Any]]
) -> dict[str, Any]:
    """The fixture: where it comes from, its notes, and its cases. ``sdk``
    is the package of the vendor's SDK."""
    commit, dirty = roomkit_source()
    return {
        "generator": generator,
        "roomkit_version": roomkit.__version__,
        "roomkit_commit": commit,
        "roomkit_dirty": dirty,
        f"{sdk.__name__}_version": sdk.__version__,
        "pydantic_version": pydantic.__version__,
        "httpx2_version": httpx2.__version__,
        "python_version": platform.python_version(),
        "clock": CLOCK,
        "notes": notes,
        "questioned": [case["name"] for case in cases if "questioned" in case],
        "cases": cases,
    }


def keys_stay_apart(text: str, request_keys: Iterable[str]) -> None:
    """Refuse a fixture that writes a request's key outside ``request_api_key``."""
    for key in request_keys:
        if key in text.replace(f'"request_api_key": "{key}"', ""):
            raise AssertionError("a request's key is written outside request_api_key")


def write(
    path: Path,
    repository: Path,
    fixture: dict[str, Any],
    sdk: ModuleType,
    request_keys: Iterable[str] = (REQUEST_KEY,),
) -> None:
    """Write ``fixture`` at ``path``, and say what was written."""
    # A number JSON does not name must not reach the fixture.
    text = json.dumps(fixture, indent=1, allow_nan=False)
    keys_stay_apart(text, request_keys)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text + "\n", encoding="utf-8")
    cases = fixture["cases"]
    failed = sum("error" in case["expected"] for case in cases)
    print(
        f"Wrote {path.relative_to(repository)}: {len(cases)} cases, {failed} that fail, "
        f"{len(fixture['questioned'])} questioned, from roomkit {roomkit.__version__}, "
        f"{sdk.__name__} {sdk.__version__}, httpx2 {httpx2.__version__}, "
        f"Python {platform.python_version()}"
    )
