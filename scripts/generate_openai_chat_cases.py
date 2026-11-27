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
import datetime
from functools import partial
from pathlib import Path
from typing import Any

import httpx2
import openai
from openai import _base_client as sdk
from openai_chat_cases import CASES
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.ai.reasoning import reasoning_floor
from roomkit.providers.cerebras import CerebrasAIProvider, CerebrasConfig
from roomkit.providers.deepseek import DeepSeekAIProvider, DeepSeekConfig
from roomkit.providers.kimi import KimiAIProvider, KimiConfig
from roomkit.providers.litellm import LiteLLMAIProvider, LiteLLMConfig
from roomkit.providers.litellm import ai as litellm_ai
from roomkit.providers.meta import MetaAIProvider, MetaConfig
from roomkit.providers.minimax import MiniMaxAIProvider, MiniMaxConfig
from roomkit.providers.openai import OpenAIAIProvider, OpenAIConfig
from roomkit.providers.openrouter import OpenRouterAIProvider, OpenRouterConfig
from roomkit.providers.qwen import QwenAIProvider, QwenConfig
from roomkit.providers.xai import XAIAIProvider, XAIConfig
from vendor_chat_cases import VENDOR_CASES
from wire_capture import (
    NOW,
    Case,
    MintedIds,
    Wire,
    answered_texts,
    case_form,
    error_form,
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
FIXTURE = REPOSITORY / "crates/roomkit-providers-openai/tests/fixtures/python_chat_cases.json"

# The tags of a model's catalogue entry the provider reads on Chat Completions.
READ_TAGS = (
    "responses_only",
    "chat_tools_refused",
    "tools_reasoning_none",
    "tools_reasoning_effort",
)

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


# The headers a vendor's provider sets of its own, written where a request
# carries one: OpenRouter's attribution.
VENDOR_HEADERS = ("http-referer", "x-title")


def sent(request: httpx2.Request, case: Case) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    headers = {
        "authorization": named_key(request.headers.get("authorization")),
        "content-type": request.headers.get("content-type"),
        "accept": request.headers.get("accept"),
    }
    for name in case.config.get("default_headers") or {}:
        if name.lower() not in headers:
            headers[name.lower()] = request.headers.get(name)
    for name in VENDOR_HEADERS:
        if name in request.headers and name not in headers:
            headers[name] = request.headers[name]
    return request_form(request, headers)


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


def catalogue_of(provider: OpenAIAIProvider) -> dict[str, Any] | None:
    """What ``provider`` reads of its model's catalogue entry; ``None``
    behind a base URL, where OpenAI's provider reads none. A vendor's
    provider reads its own catalogue wherever it is pointed."""
    if type(provider) is OpenAIAIProvider and not provider._is_openai_endpoint:
        return None
    entry = provider.catalog_entry()
    tags = [tag for tag in READ_TAGS if entry is not None and tag in entry.capabilities]
    return {
        "model": provider.model_name,
        "known": entry is not None,
        "tags": tags,
        "reasoning_floor": reasoning_floor(entry),
    }


async def answered_by_python(
    case: Case, wire: Wire, context: AIContext, classes: tuple[Any, Any]
) -> dict[str, Any]:
    """What the provider of ``case``, of ``classes`` (its class and its
    configuration's), returns over ``wire`` and what it read of the
    catalogue, or the error that kept it from being built."""
    provider_class, config_class = classes
    transport = httpx2.MockTransport(wire)
    try:
        provider = provider_class(config_class(**case.config), transport=transport)
    except openai.OpenAIError as unbuilt:
        return {"expected": {"error": error_form(unbuilt)}, "catalogue": None, "vision": None}
    wire.record_waits(provider._client, sdk)
    try:
        expected = await outcome(provider, case.call, context)
        vision = provider.supports_vision
        return {"expected": expected, "catalogue": catalogue_of(provider), "vision": vision}
    finally:
        await provider.close()


async def run(
    case: Case, classes: tuple[Any, Any] = (OpenAIAIProvider, OpenAIConfig)
) -> dict[str, Any]:
    """One case, as the fixture writes it."""
    minted = tool_calls.uuid4 = MintedIds()
    wire = Wire(case.name, case.replies, partial(sent, case=case))
    context = AIContext.model_validate(case.request)
    ran = await answered_by_python(case, wire, context, classes)
    words = partial(words_of, sent=answered_texts(case.replies))
    turn = turn_form(case.request, context, wire, ran["expected"], words, minted)
    told = {"catalogue": ran["catalogue"], "supports_vision": ran["vision"]}
    return case_form(case, told, [turn])


# The vendors whose RoomKit Python provider derives from OpenAIAIProvider,
# by the name of their table in vendor_chat_cases.VENDOR_CASES: the class of
# their provider and of its configuration.
VENDORS: dict[str, tuple[Any, Any]] = {
    "deepseek": (DeepSeekAIProvider, DeepSeekConfig),
    "kimi": (KimiAIProvider, KimiConfig),
    "minimax": (MiniMaxAIProvider, MiniMaxConfig),
    "xai": (XAIAIProvider, XAIConfig),
    "cerebras": (CerebrasAIProvider, CerebrasConfig),
    "qwen": (QwenAIProvider, QwenConfig),
    "meta": (MetaAIProvider, MetaConfig),
    "openrouter": (OpenRouterAIProvider, OpenRouterConfig),
    "litellm": (LiteLLMAIProvider, LiteLLMConfig),
}


class FixtureDay:
    """``date`` as LiteLLM's provider reads it, its day the fixture's: it
    dates the rates it reads live from its proxy with ``date.today()``,
    which the stopped clock of the SDK does not reach."""

    @staticmethod
    def today() -> datetime.date:
        return datetime.datetime.fromtimestamp(NOW, tz=datetime.UTC).date()


def main() -> None:
    # The SDK reads its key, its base URL and more from the environment when
    # a client is given none: a case is what its configuration says.
    prepare(sdk, "OPENAI_")
    litellm_ai.date = FixtureDay  # type: ignore[misc]
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    fixture = fixture_of("scripts/generate_openai_chat_cases.py", openai, NOTES, cases)
    write(FIXTURE, REPOSITORY, fixture, openai)
    for vendor, (provider_class, config_class) in VENDORS.items():
        table = VENDOR_CASES[vendor]
        unique_names(table)
        written = [asyncio.run(run(case, (provider_class, config_class))) for case in table]
        named = config_class.__name__
        notes = {**NOTES, "config": f"The keyword arguments of {named}. The key is a placeholder."}
        fixture = fixture_of("scripts/generate_openai_chat_cases.py", openai, notes, written)
        write(FIXTURE.with_name(f"python_{vendor}_cases.json"), REPOSITORY, fixture, openai)


if __name__ == "__main__":
    main()
