#!/usr/bin/env python3
"""Write the wire cases of RoomKit's Python Gemini provider on Vertex AI.

Each case runs ``GeminiVertexProvider`` once, on Vertex's
``streamGenerateContent`` or on its model listing, its identity a
service-account key, against a fake of the wire (nothing leaves this
script), and records:

- ``config``: the keyword arguments of its ``GeminiVertexConfig``, a key
  written as ``vertex_cases.py`` says (``KEYED``);
- ``request``: the contract's JSON form of the request;
- ``exchanges``: each HTTP request Python sent, in order, and the response
  the fake gave it: the token request google-auth sends on ``requests``,
  then the generation google-genai sends on ``httpx``;
- ``expected``: what Python returned, or the error it raised, and
  ``compare``, how a replay compares that error.

The fake sits under every client at once: httpx's network transport and
the transport adapter of ``requests``. The clock google-auth reads is stopped, so an
assertion is signed alike at every run, and the key is the test key of the
fixtures.

Run it with a Python interpreter that has ``roomkit`` and its ``gemini``
extra installed.
"""

from __future__ import annotations

import asyncio
import base64
import datetime
import json
import os
import tempfile
import warnings
from collections.abc import Iterator
from contextlib import contextmanager
from functools import partial
from pathlib import Path
from typing import Any
from urllib.parse import parse_qsl

import httpx
import requests
import requests.adapters
from google import genai
from google.auth import _exponential_backoff as auth_backoff
from google.auth import _helpers as auth_clock
from google.auth.compute_engine import _metadata as metadata_server
from google.genai import _api_client as sdk
from roomkit.providers.ai import AIContext, tool_calls
from roomkit.providers.gemini.vertex import GeminiVertexConfig, GeminiVertexProvider
from vertex_cases import CASES
from wire_capture import (
    NOW,
    Case,
    MintedIds,
    Reply,
    Turn,
    Wire,
    error_form,
    answered_texts,
    case_form,
    data_lines,
    fixture_of,
    outcome,
    prepare,
    turn_form,
    unique_names,
    write,
)

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURES = REPOSITORY / "crates/roomkit-providers-gemini/tests/fixtures"
FIXTURE = FIXTURES / "python_vertex_cases.json"
TEST_KEY = FIXTURES / "test-only-rsa-pkcs8.der"

# The headers of a request the fixture keeps.
KEPT_HEADERS = ("authorization", "content-type", "accept", "metadata-flavor", "x-goog-user-project")

# The classes google-genai raises for an answer whose status is an error.
SDK_ERRORS = {"APIError", "ClientError", "ServerError", "UnknownApiResponseError"}

# The classes google-auth raises for a token it could not get.
AUTH_ERRORS = {"RefreshError", "TransportError", "DefaultCredentialsError"}

# The classes the HTTP clients raise for a body they could not read.
TRANSPORT_ERRORS = {"RemoteProtocolError", "ReadTimeout", "ReadError", "ConnectError"}

NOTES = {
    "environment": (
        "Every GOOGLE_*, GEMINI_*, CLOUDSDK_* and GCE_* variable of the environment is "
        "removed before a case runs: google-genai and google-auth read a project, a "
        "location and credentials from them."
    ),
    "versions": "The versions the fixture was written from.",
    "config": (
        "The keyword arguments of GeminiVertexConfig. service_account_json is written "
        "{\"KEYED\": members}: the members of a key file but its private key, which is "
        "the PEM of test-only-rsa-pkcs8.der beside this fixture. A replay builds the same "
        "key: json.dumps(members + private_key)."
    ),
    "request": "AIContext dumped with model_dump(mode='json'), its ten request fields.",
    "exchanges.request": (
        "What Python sent, in order, on either client: the method, the whole URL, the body "
        "(JSON with its members in the order sent, or {form: [[name, value], ...]} for a "
        "form, in order), and five headers: authorization, content-type, accept, "
        "metadata-flavor and x-goog-user-project, null when not sent. The token request of a key comes from "
        "google-auth, on requests; its other headers (x-goog-api-client, which names "
        "Python's version, user-agent) are left out, as are google-genai's. timeouts "
        "holds, in seconds, how long the request may wait to connect, for a read, for a "
        "write and for a connection of the pool. The assertion of a key is signed at the "
        "fixture's clock, 2026-01-01T00:00:00Z, with the test key: a replay signs the same."
    ),
    "exchanges.wait": (
        "Where google-auth tried a token request again, the wait it took first, in seconds, "
        "its random part drawn at the centre of its range (it draws within 10% of it)."
    ),
    "exchanges.response": (
        "What the fake answered: status and headers with a body under json, under text for "
        "a body that is no JSON, or under chunks for a body read in several reads. A "
        "failure in place of a response is under failure: no answer came."
    ),
    "expected": (
        "What Python returned: response for generate, events for "
        "generate_structured_stream, models for list_models. A failure is under error: "
        "its class, its message, its text, the attributes of a ProviderError, and "
        "raised_from, the class of the error it was raised from, if any."
    ),
    "compare": (
        "On a call that fails, how a replay compares the failure: exactly lists what it "
        "reproduces as written; message says whose words the message holds: roomkit "
        "(compared), server (compared), sdk (google-genai's words: not compared), auth "
        "(google-auth's words: not compared), python_json, transport, python (not "
        "compared)."
    ),
    "sdk": (
        "Where a case says sdk, google-genai decided the behaviour, not RoomKit: here the "
        "host a location is asked on."
    ),
}


def test_key_pem() -> str:
    """The PEM of the test key, as a key file writes it."""
    der = base64.b64encode(TEST_KEY.read_bytes()).decode()
    lines = [der[at : at + 64] for at in range(0, len(der), 64)]
    return "-----BEGIN PRIVATE KEY-----\n" + "\n".join(lines) + "\n-----END PRIVATE KEY-----\n"


def written(content: Any) -> str:
    """A file's content as the fixture gives it: a key file written whole
    (``{"KEYED": members}``), any other JSON as it is."""
    if isinstance(content, dict) and "KEYED" in content:
        return json.dumps({**content["KEYED"], "private_key": test_key_pem()})
    return json.dumps(content)


def built(config: dict[str, Any]) -> dict[str, Any]:
    """``config`` with its key written whole, as a key file holds it, and
    without the environment the case runs in."""
    config = {name: value for name, value in config.items() if name != "ENVIRONMENT"}
    keyed = config.get("service_account_json")
    if not isinstance(keyed, dict):
        return config
    return {**config, "service_account_json": written(keyed)}


@contextmanager
def environment_of(case: Case) -> Iterator[str]:
    """While it lasts, the environment `case` runs in (``ENVIRONMENT`` of
    its configuration): its files in a directory of their own, which is
    also its HOME, its variables set, ``{dir}`` in them naming that
    directory, and whether the machine says it is a Google Compute Engine
    one. Gives the directory."""
    stated = case.config.get("ENVIRONMENT", {})
    kept = dict(os.environ)
    product_file, host = metadata_server._GCE_PRODUCT_NAME_FILE, metadata_server._GCE_METADATA_HOST
    with tempfile.TemporaryDirectory() as directory:
        for name, content in stated.get("files", {}).items():
            path = Path(directory) / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(written(content), encoding="utf-8")
        # No case reads the home of the machine it is captured on: gcloud's
        # file there would be found.
        os.environ["HOME"] = directory
        for name, value in stated.get("variables", {}).items():
            os.environ[name] = value.replace("{dir}", directory)
        product = Path(directory) / "product_name"
        if "product_name" in stated:
            product.write_text(stated["product_name"], encoding="utf-8")
        metadata_server._GCE_PRODUCT_NAME_FILE = str(product)
        # google-auth reads the host once, when it is imported; the address
        # it pings, at each ping.
        metadata_server._GCE_METADATA_HOST = os.environ.get(
            "GCE_METADATA_HOST", os.environ.get("GCE_METADATA_ROOT", host)
        )
        try:
            yield directory
        finally:
            os.environ.clear()
            os.environ.update(kept)
            metadata_server._GCE_PRODUCT_NAME_FILE = product_file
            metadata_server._GCE_METADATA_HOST = host


def from_requests(sent: requests.PreparedRequest, timeout: Any) -> httpx.Request:
    """A request google-auth sends on ``requests``, as it goes on the wire
    (the session's own headers merged in), written as httpx writes one."""
    data = sent.body
    content = data.encode() if isinstance(data, str) else data
    request = httpx.Request(sent.method, sent.url, headers=dict(sent.headers), content=content)
    request.extensions["timeout"] = {
        "connect": timeout,
        "read": timeout,
        "write": timeout,
        "pool": timeout,
    }
    return request


def to_requests(answered: httpx.Response, sent: requests.PreparedRequest) -> requests.Response:
    response = requests.Response()
    response.status_code = answered.status_code
    response.headers.update(answered.headers)
    response._content = answered.read()
    response.url = sent.url
    response.request = sent
    return response


@contextmanager
def under_every_client(wire: Wire) -> Iterator[None]:
    """While it lasts, ``wire`` answers every request httpx and requests
    would put on the network, and google-auth's clock is stopped."""
    send = httpx.AsyncHTTPTransport.handle_async_request
    adapter_send = requests.adapters.HTTPAdapter.send
    utcnow = auth_clock.utcnow

    async def answered(transport: httpx.AsyncHTTPTransport, request: httpx.Request) -> Any:
        await request.aread()
        return await wire(request)

    def asked(adapter: Any, prepared: requests.PreparedRequest, **kwargs: Any) -> Any:
        request = from_requests(prepared, kwargs.get("timeout"))
        try:
            return to_requests(wire.answer(request), prepared)
        except httpx.ConnectError as failed:
            raise requests.exceptions.ConnectionError(str(failed)) from None
        except httpx.ReadTimeout as failed:
            raise requests.exceptions.ReadTimeout(str(failed)) from None

    def stopped() -> datetime.datetime:
        return datetime.datetime.fromtimestamp(NOW, datetime.UTC).replace(tzinfo=None)

    class Waits:
        """google-auth's time, its waits between tries recorded, not taken."""

        def sleep(self, seconds: float) -> None:
            wire.exchanges[-1]["wait"] = {"seconds": seconds, "from": "google-auth's backoff"}

        def __getattr__(self, name: str) -> Any:
            return getattr(time_of_the_backoff, name)

    class Centred:
        """google-auth's random, each jitter drawn at the centre of its range."""

        def uniform(self, low: float, high: float) -> float:
            return (low + high) / 2

        def __getattr__(self, name: str) -> Any:
            return getattr(random_of_the_backoff, name)

    time_of_the_backoff, random_of_the_backoff = auth_backoff.time, auth_backoff.random
    httpx.AsyncHTTPTransport.handle_async_request = answered
    requests.adapters.HTTPAdapter.send = asked
    auth_clock.utcnow = stopped
    auth_backoff.time, auth_backoff.random = Waits(), Centred()
    try:
        yield
    finally:
        httpx.AsyncHTTPTransport.handle_async_request = send
        requests.adapters.HTTPAdapter.send = adapter_send
        auth_clock.utcnow = utcnow
        auth_backoff.time, auth_backoff.random = time_of_the_backoff, random_of_the_backoff


def body_of(request: httpx.Request) -> Any:
    if not request.content:
        return None
    kind = request.headers.get("content-type", "")
    if kind.startswith("application/x-www-form-urlencoded"):
        return {"form": [list(pair) for pair in parse_qsl(request.content.decode())]}
    return json.loads(request.content)


def sent(request: httpx.Request) -> dict[str, Any]:
    """``request`` as the fixture writes it."""
    return {
        "method": request.method,
        "url": str(request.url),
        "headers": {name: request.headers.get(name) for name in KEPT_HEADERS},
        "timeouts": request.extensions["timeout"],
        "body": body_of(request),
    }


def words_of(error: dict[str, Any], replies: tuple[Reply, ...]) -> str:
    """Whose words the message of ``error`` holds (``NOTES["compare"]``)."""
    cause, message = error["raised_from"], error["message"]
    if error["class"] in SDK_ERRORS:
        return "sdk"
    if error["class"] in AUTH_ERRORS or cause in AUTH_ERRORS:
        return "auth"
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


async def run_turn(
    provider: GeminiVertexProvider, case: Case, turn: Turn, minted: MintedIds
) -> dict[str, Any]:
    wire = Wire(case.name, turn.replies, sent, http=httpx)
    context = AIContext.model_validate(turn.request)
    with under_every_client(wire):
        expected = await outcome(provider, case.call, context)
    words = partial(words_of, replies=turn.replies)
    return turn_form(turn.request, context, wire, expected, words, minted)


async def run(case: Case) -> dict[str, Any]:
    """One case, as the fixture writes it, in its environment: the
    directory of its files written ``{dir}``."""
    with environment_of(case) as directory:
        form = await run_in(case)
    return json.loads(json.dumps(form).replace(directory, "{dir}"))


async def run_in(case: Case) -> dict[str, Any]:
    minted = tool_calls.uuid4 = MintedIds()
    wire = Wire(case.name, case.replies, sent, http=httpx)
    try:
        with under_every_client(wire):
            provider = GeminiVertexProvider(GeminiVertexConfig(**built(case.config)))
    except Exception as refused:
        # Python refuses the configuration when the provider is built, after
        # what it asked then.
        written = {"name": case.name, "kinds": sorted(case.kinds), "call": case.call}
        written.update({"config": case.config, "request": case.request})
        written["exchanges"] = wire.exchanges
        return {**written, "unbuilt": True, "expected": {"error": error_form(refused)}}
    # What the provider asked as it was built, its replies taken: the call
    # goes on from there, and its exchanges start with those.
    early = wire.exchanges
    try:
        turns = [
            await run_turn(provider, case, turn, minted)
            for turn in (*case.before, Turn(case.request, case.replies[len(early) :]))
        ]
        turns[-1]["exchanges"] = [*early, *turns[-1]["exchanges"]]
        return case_form(case, {"supports_vision": provider.supports_vision}, turns)
    finally:
        await provider.close()


def main() -> None:
    prepare(sdk, "GOOGLE_")
    for prefix in ("GEMINI_", "CLOUDSDK_", "GCE_", "NO_GCE_CHECK"):
        for name in [name for name in os.environ if name.startswith(prefix)]:
            del os.environ[name]
    warnings.filterwarnings("ignore", category=SyntaxWarning)
    unique_names(CASES)
    cases = [asyncio.run(run(case)) for case in CASES]
    generator = "scripts/generate_vertex_cases.py"
    fixture = fixture_of(generator, genai, NOTES, cases, http=httpx)
    write(FIXTURE, REPOSITORY, fixture, genai, http=httpx)


if __name__ == "__main__":
    main()
