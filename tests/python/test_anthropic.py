"""Anthropic's provider as a RoomKit program holds it: its configuration,
what it says of itself, its calls against a local fake of the endpoint, in
RoomKit's models and RoomKit's exceptions, and a RoomKit agent over it.

Everything is observed from outside: what the fake was sent, what a call
returned or raised, what RoomKit's telemetry recorded.

A panic of the native code cannot be provoked through a real provider. The
tests of the shared part hold it (``test_provider.py``), on a scripted
provider that enters the native module where every provider does: a panic in
a call, while a stream is read, while a provider or a configuration is built.
"""

from __future__ import annotations

import asyncio
import importlib.util
import json
import os
import socket
import subprocess
import sys
import time
import traceback
import unittest
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any
from unittest import mock

from fake_anthropic import (
    OTHER_SIGNATURE,
    REDACTED,
    SIGNATURE,
    answers,
    call,
    calls,
    declines,
    local_config,
    models,
    opens_and_waits,
    redacted,
    refuses,
    says,
    text,
    thinking,
)
from fake_endpoint import (
    DEADLINE,
    JSON,
    NO_OPENSSL,
    OPENSSL,
    Authority,
    Fake,
    Refused,
    Silent,
    Tunnel,
    Whole,
)
from pydantic import ValidationError
from roomkit import (
    AIChannel,
    ChannelCategory,
    HookExecution,
    HookTrigger,
    InboundMessage,
    RoomKit,
    TextContent,
    WebSocketChannel,
)
from roomkit.channels import StreamChunk, StreamEnd, StreamStart
from roomkit.providers.ai import (
    AIContext,
    AIMessage,
    AIProvider,
    AIResponse,
    AIThinkingPart,
    AITool,
    AIToolCallPart,
    AIToolResultPart,
    ModelInfo,
    ProviderError,
    ResponseSchemaError,
    StreamDone,
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
    StreamUsage,
    round_parts,
    thinking_parts_of,
)
from roomkit.providers.anthropic import AnthropicAIProvider as RoomKitProvider
from roomkit.providers.anthropic import AnthropicConfig as RoomKitConfig
from roomkit.telemetry import MockTelemetryProvider
from roomkit_providers import _native
from roomkit_providers.llm import _config
from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
FORMS = ROOT / "crates/roomkit-providers-anthropic/tests/fixtures/python_config_forms.json"

# Placeholders, a secret in each place a configuration or a request holds one.
KEY = "placeholder-not-a-key"
ONE_REQUEST_KEY = "placeholder-of-one-request"
HEADER = "placeholder-header-value"
PASSWORD = "placeholder-password"
TOKEN = "placeholder-token"
SECRETS = (KEY, ONE_REQUEST_KEY, HEADER, PASSWORD, TOKEN)

# A model Anthropic's own endpoint takes no temperature and no thinking
# budget from, and one it takes both from.
MODERN = "claude-opus-5-5"
OLD = "claude-sonnet-4-6"

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
ORDER = AITool(
    name="lookup_order",
    description="Finds an order by its number.",
    parameters={
        "type": "object",
        "properties": {"number": {"type": "string"}},
        "required": ["number"],
    },
)
SCHEMA = {
    "type": "object",
    "properties": {"city": {"type": "string"}},
    "required": ["city"],
    "additionalProperties": False,
}
# The two shapes a turn that thinks is asked in.
ADAPTIVE = {"type": "adaptive", "display": "summarized"}
BUDGET = {"type": "enabled", "budget_tokens": 2048}

# What a provider says of itself.
SAID = (
    "name",
    "model_name",
    "supports_vision",
    "supports_streaming",
    "supports_structured_streaming",
    "supports_response_schema",
    "supports_response_schema_with_tools",
    "supports_deferred_tools",
    "context_window",
)
# Configurations a provider is built with: a model of the catalogue that is
# modern, old, or older than the tools held unseen, a model the catalogue
# does not know, another server, Anthropic's endpoint written out, a switch
# stated, and a credential that comes as a header.
BUILT = [
    {"api_key": KEY, "model": MODERN},
    {"api_key": KEY, "model": OLD},
    {"api_key": KEY, "model": "claude-opus-4-1"},
    {"api_key": KEY, "model": "claude-next-1"},
    {"api_key": KEY, "model": "local-model"},
    {"api_key": KEY, "model": MODERN, "base_url": "https://llm.example"},
    {"api_key": KEY, "model": MODERN, "base_url": "https://api.anthropic.com/v1/messages"},
    {"api_key": KEY, "model": MODERN, "use_adaptive_thinking": False},
    {"api_key": "", "model": "local-model", "extra_headers": {"Authorization": "Bearer other"}},
]
# What every refused configuration below holds, a secret in each place one
# goes.
HELD = {
    "api_key": KEY,
    "model": "local-model",
    "base_url": f"https://team:{PASSWORD}@llm.example",
    "extra_headers": {"X-Team-Token": HEADER},
}
# Configurations RoomKit's class takes and no provider is built with.
UNBUILT = {
    "a time below zero": {"timeout": -1.0},
    "a connection time below zero": {"connect_timeout": -0.5},
    "a time that is no number": {"timeout": float("nan")},
    "a time without end": {"timeout": float("inf")},
    "retries below zero": {"max_retries": -1},
    "tokens below zero": {"max_tokens": -5},
    "tokens past the largest": {"max_tokens": 2**40},
    "a base URL with a query": {"base_url": f"https://llm.example/?token={TOKEN}"},
    "a base URL with a fragment": {"base_url": f"https://llm.example/#{TOKEN}"},
    "a base URL with white space": {"base_url": f"https://team:{PASSWORD}@llm.example/a b"},
    "a base URL of another scheme": {"base_url": f"ftp://team:{PASSWORD}@llm.example/{TOKEN}"},
    "a base URL that is none": {"base_url": f"{TOKEN} is no URL"},
    "a header's value on two lines": {"extra_headers": {"X-Team-Token": f"{HEADER}\nmore"}},
    "a header's name with a space": {"extra_headers": {"X Team Token": HEADER}},
}

NO_SDK = (
    "RoomKit's own Anthropic provider is built on the anthropic SDK, which is not installed: "
    "what it says of itself is compared where the SDK is"
)
# Why a test that names Anthropic's own endpoint runs only where the tests
# are given no network (`scripts/no-network.sh`): nothing it sends can leave.
NO_NAMESPACE = (
    "this test asks for Anthropic's own endpoint, and runs only where no network is reachable"
)
# What a provider reads of its environment, beside the names that end in
# `_proxy`, in any case.
READ = ("request_method", "ssl_cert_file", "ssl_cert_dir")


def switches(config: AnthropicConfig | RoomKitConfig) -> tuple[bool, bool]:
    """The two switches a model decides: whether thinking is asked as
    adaptive, and whether a temperature is sent."""
    return config.use_adaptive_thinking, config.supports_custom_temperature


def said(provider: AIProvider) -> dict[str, object]:
    return {name: getattr(provider, name) for name in SAID}


def blocks(message: dict[str, Any]) -> list[dict[str, Any]]:
    """The content blocks of a message that was sent, without the marks of
    the prompt cache: a text sent as it is reads as one block of text."""
    content = message["content"]
    if isinstance(content, str):
        return [{"type": "text", "text": content}]
    return [
        {name: member for name, member in block.items() if name != "cache_control"}
        for block in content
    ]


def lent(key: str = ONE_REQUEST_KEY, role: str = "user") -> AIContext:
    """A context whose host lent it a key for its one request."""
    return AIContext(messages=[AIMessage(role=role, content="Hello")], metadata={"api_key": key})


def written(error: BaseException) -> str:
    """Everything a traceback of `error` writes with its frames' locals, the
    frames of this file left out: a test holds what it tests with."""
    held = traceback.TracebackException.from_exception(error, capture_locals=True)
    seen, chain = [], [held]
    while chain:
        link = chain.pop()
        seen.append(str(link))
        seen += [
            f"{frame.filename}:{frame.name}:{frame.locals}"
            for frame in link.stack
            if frame.filename != __file__
        ]
        chain += [other for other in (link.__cause__, link.__context__) if other is not None]
    return "\n".join(seen)


def routed() -> bool:
    """Whether a route leads off the machine, asked of the kernel: a UDP
    socket connected to an address kept for documentation needs a route
    there, and sends nothing. No variable of the environment is believed."""
    for family, address in ((socket.AF_INET, "192.0.2.1"), (socket.AF_INET6, "2001:db8::1")):
        try:
            with socket.socket(family, socket.SOCK_DGRAM) as probe:
                probe.connect((address, 9))
        except OSError:
            continue
        return True
    return False


@contextmanager
def environment(**variables: str) -> Iterator[None]:
    """The environment a provider is built in: this process's, without what
    a provider reads of it, and with `variables`. It is put back after."""
    kept = dict(os.environ)
    for name in kept:
        if name.lower().endswith("_proxy") or name.lower() in READ:
            del os.environ[name]
    os.environ.update(variables)
    try:
        yield
    finally:
        os.environ.clear()
        os.environ.update(kept)


class ConfigTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # What RoomKit's configuration made of each argument when the
        # crate's fixture was written: the arguments are read here, and
        # given again to both classes.
        cls.forms = json.loads(FORMS.read_text(encoding="utf-8"))

    def test_the_fields_and_the_defaults_are_those_of_roomkit_s_configuration(self) -> None:
        def fields(config: type) -> dict:
            return {
                name: (field.annotation, field.default, field.is_required())
                for name, field in config.model_fields.items()
            }

        self.assertEqual(fields(AnthropicConfig), fields(RoomKitConfig))

    def test_a_configuration_is_read_as_roomkit_s_reads_it(self) -> None:
        # Every model's profile, each member stated, values pydantic
        # converts, an argument neither class knows.
        taken = [*self.forms["examples"], *self.forms["converted"]]
        self.assertGreaterEqual(len(taken), 70)
        for entry in taken:
            members = entry["given"]
            with self.subTest(configuration=entry["name"]):
                own, roomkit_s = AnthropicConfig(**members), RoomKitConfig(**members)
                self.assertEqual(own.model_dump(), roomkit_s.model_dump())
                self.assertEqual(own.model_fields_set, roomkit_s.model_fields_set)
                self.assertEqual(
                    own.api_key.get_secret_value(), roomkit_s.api_key.get_secret_value()
                )

    def test_a_configuration_is_refused_as_roomkit_s_refuses_it(self) -> None:
        def refusal(config: type, members: dict) -> list[tuple]:
            with self.assertRaises(ValidationError) as refused:
                config(**members)
            return sorted((error["loc"], error["type"]) for error in refused.exception.errors())

        refused = [entry for entry in self.forms["rejected"] if isinstance(entry["given"], dict)]
        self.assertGreaterEqual(len(refused), 20)
        for entry in refused:
            members = entry["given"]
            with self.subTest(configuration=entry["name"]):
                self.assertEqual(refusal(AnthropicConfig, members), refusal(RoomKitConfig, members))
                # The refusal says what is refused and never shows the key.
                with self.assertRaises(ValidationError) as failed:
                    AnthropicConfig(**members)
                self.assertNotIn(KEY, str(failed.exception))

    def test_the_switches_a_model_decides_are_decided_as_roomkit_s_configuration_does(
        self,
    ) -> None:
        elsewhere = {"base_url": "https://llm.example"}
        written_out = {"base_url": "https://api.anthropic.com/v1/messages"}
        both = {"use_adaptive_thinking": False, "supports_custom_temperature": True}
        # A model, what is stated beside it, and what is then decided:
        # whether thinking is adaptive, whether a temperature is sent.
        cases = [
            (MODERN, {}, (True, False)),
            (OLD, {}, (False, True)),
            ("local-model", {}, (False, True)),
            # Anthropic's own endpoint, however it is written.
            (MODERN, written_out, (True, False)),
            # Behind another base URL no model is profiled.
            (MODERN, elsewhere, (False, True)),
            # A switch that is stated stays as stated, and the other is decided.
            (MODERN, {"use_adaptive_thinking": False}, (False, False)),
            (MODERN, {"supports_custom_temperature": True}, (True, True)),
            (MODERN, both, (False, True)),
            (OLD, {"use_adaptive_thinking": True}, (True, True)),
            (OLD, {"supports_custom_temperature": False}, (False, False)),
            (MODERN, {**elsewhere, "use_adaptive_thinking": True}, (True, True)),
            (MODERN, {**elsewhere, "supports_custom_temperature": False}, (False, False)),
        ]
        for model, stated, decided in cases:
            members = {"api_key": KEY, "model": model, **stated}
            with self.subTest(model=model, stated=stated):
                own, roomkit_s = AnthropicConfig(**members), RoomKitConfig(**members)
                self.assertEqual(switches(own), decided)
                self.assertEqual(switches(own), switches(roomkit_s))
                self.assertEqual(own.model_fields_set, roomkit_s.model_fields_set)

    def test_a_base_url_is_kept_without_the_path_of_the_messages_api(self) -> None:
        kept = {
            "https://llm.example/anthropic/v1/messages": "https://llm.example/anthropic",
            " https://llm.example/anthropic/V1/Messages// ": "https://llm.example/anthropic",
            "https://llm.example/": "https://llm.example",
            # A `/v1` alone is left: a gateway may route on it.
            "https://llm.example/v1/": "https://llm.example/v1",
        }
        for given, expected in kept.items():
            for config in (AnthropicConfig, RoomKitConfig):
                with self.subTest(base_url=given, config=config.__module__):
                    held = config(api_key=KEY, model="local-model", base_url=given)
                    self.assertEqual(held.base_url, expected)

    def test_a_base_url_is_read_as_roomkit_s_configuration_reads_it(self) -> None:
        endpoints = self.forms["endpoints"]
        refused = [url["base_url"] for url in endpoints if url["anthropic"] == "refused"]
        self.assertEqual(len(refused), 4)
        for base_url in refused:
            members = {"api_key": KEY, "model": MODERN, "base_url": base_url}
            with self.subTest(base_url=base_url):
                outcomes = []
                for config in (AnthropicConfig, RoomKitConfig):
                    with self.assertRaises(ValidationError) as failed:
                        config(**members)
                    errors = failed.exception.errors()
                    outcomes.append([(error["loc"], error["type"]) for error in errors])
                self.assertEqual(outcomes[0], outcomes[1])
                # The refusal shows neither the URL nor the key.
                with self.assertRaises(ValidationError) as failed:
                    AnthropicConfig(**members)
                self.assertNotIn(KEY, str(failed.exception))
                self.assertNotIn(base_url, str(failed.exception))
        taken = [url for url in endpoints if url["anthropic"] != "refused"]
        self.assertGreaterEqual(len(taken), 60)
        for url in taken:
            members = {"api_key": KEY, "model": MODERN, "base_url": url["base_url"]}
            with self.subTest(base_url=url["base_url"]):
                own, roomkit_s = AnthropicConfig(**members), RoomKitConfig(**members)
                self.assertEqual(own.base_url, roomkit_s.base_url)
                self.assertEqual(own.base_url, url["kept"])
                # Whether the URL is Anthropic's own endpoint is what
                # decides the switches of a modern model.
                self.assertEqual(switches(own), switches(roomkit_s))
                self.assertEqual(switches(own), (url["anthropic"], not url["anthropic"]))

    def test_a_positional_argument_is_refused(self) -> None:
        with self.assertRaises(TypeError):
            AnthropicConfig(KEY, MODERN)

    def test_repr_shows_no_key_no_header_value_and_no_credentials_of_the_url(self) -> None:
        config = AnthropicConfig(
            api_key=KEY,
            model=MODERN,
            base_url=f"https://team:{PASSWORD}@llm.example:8443/placeholder-path?t={TOKEN}",
            extra_headers={"Authorization": f"Basic {HEADER}"},
        )
        for shown in (repr(config), str(config)):
            self.assertNotIn(KEY, shown)
            self.assertNotIn(PASSWORD, shown)
            self.assertNotIn("team", shown)
            self.assertNotIn(HEADER, shown)
            # A gateway's token rides the path or the query of its URL.
            self.assertNotIn("placeholder-path", shown)
            self.assertNotIn(TOKEN, shown)
            self.assertIn("https://llm.example:8443", shown)
            self.assertIn("Authorization", shown)
        plain = repr(AnthropicConfig(api_key=KEY, model="m", base_url="http://localhost:1/a"))
        self.assertIn("base_url='http://localhost:1'", plain)
        self.assertIn("base_url=None", repr(AnthropicConfig(api_key=KEY, model="m")))
        unread = repr(AnthropicConfig(api_key=KEY, model="m", base_url="placeholder-path, no URL"))
        self.assertNotIn("placeholder-path", unread)
        # What crosses to the provider is not what is shown.
        self.assertIn(HEADER, config.model_dump_json())


class ProviderTests(unittest.TestCase):
    def refusal(self, config: Any) -> tuple[BaseException, str]:
        """What refuses `config`, and what its traceback writes: read while
        its frames still hold their locals, which `assertRaises` clears."""
        try:
            Anthropic(config)
        except (ValueError, TypeError, ProviderError) as refused:
            return refused, written(refused)
        self.fail("the configuration was not refused")

    def test_the_provider_is_roomkit_s_and_says_what_roomkit_s_own_says(self) -> None:
        provider = Anthropic(AnthropicConfig(api_key=KEY, model=MODERN))
        self.assertIsInstance(provider, AIProvider)
        self.assertEqual(
            said(provider),
            {
                "name": "AnthropicAIProvider",
                "model_name": MODERN,
                "supports_vision": True,
                "supports_streaming": True,
                "supports_structured_streaming": True,
                "supports_response_schema": True,
                "supports_response_schema_with_tools": True,
                "supports_deferred_tools": True,
                "context_window": 1000000,
            },
        )
        # The name is the one RoomKit's own provider goes by: what RoomKit
        # records under it does not change with the provider.
        self.assertEqual(provider.name, RoomKitProvider.__name__)

    @unittest.skipUnless(importlib.util.find_spec("anthropic"), NO_SDK)
    def test_the_provider_says_what_roomkit_s_own_says_of_the_same_configuration(self) -> None:
        for members in BUILT:
            with self.subTest(members=members):
                own = Anthropic(AnthropicConfig(**members))
                roomkit_s = RoomKitProvider(RoomKitConfig(**members))
                self.assertEqual(said(own), said(roomkit_s))
                self.assertEqual(own.catalog_entry(), roomkit_s.catalog_entry())

    def test_the_catalogue_is_roomkit_s_own_in_roomkit_s_models(self) -> None:
        listed = Anthropic.available_models()
        self.assertTrue(all(isinstance(model, ModelInfo) for model in listed))
        self.assertEqual(listed, RoomKitProvider.available_models())
        provider = Anthropic(AnthropicConfig(api_key=KEY, model=MODERN))
        entry = provider.catalog_entry()
        self.assertEqual(entry, next(model for model in listed if model.id == MODERN))
        # What RoomKit prices a turn with.
        cost = entry.pricing.cost_for({"input_tokens": 1000, "output_tokens": 10})
        self.assertAlmostEqual(cost, 0.0042)
        # A model's entry and its window are read whatever the endpoint, and
        # a model the catalogue does not know has none.
        behind = Anthropic(
            AnthropicConfig(api_key=KEY, model=MODERN, base_url="https://llm.example")
        )
        self.assertEqual(behind.catalog_entry(), entry)
        self.assertEqual(
            (provider.context_window, behind.context_window), (entry.context_window,) * 2
        )
        local = Anthropic(AnthropicConfig(api_key=KEY, model="local-model"))
        self.assertIsNone(local.catalog_entry())
        self.assertIsNone(local.context_window)
        self.assertFalse(local.supports_vision)

    def test_a_tool_may_be_held_unseen_on_anthropic_s_own_endpoint_alone(self) -> None:
        # RoomKit's catalogue says which models hold a tool declared and
        # unseen. RoomKit's provider answers so on Anthropic's own endpoint,
        # and never behind another base URL.
        catalogue = RoomKitProvider.available_models()
        holding = {model.id for model in catalogue if "deferred_tools" in model.capabilities}
        self.assertIn(MODERN, holding)
        self.assertNotIn("claude-opus-4-1", holding)
        for model in [*(model.id for model in catalogue), "claude-next-1", "local-model"]:
            with self.subTest(model=model):
                own = Anthropic(AnthropicConfig(api_key=KEY, model=model))
                self.assertIs(own.supports_deferred_tools, model in holding)
        written_out = "https://api.anthropic.com/v1/messages"
        for base_url, holds in ((written_out, True), ("https://llm.example", False)):
            with self.subTest(base_url=base_url):
                config = AnthropicConfig(api_key=KEY, model=MODERN, base_url=base_url)
                self.assertIs(Anthropic(config).supports_deferred_tools, holds)

    def test_roomkit_s_own_configuration_builds_the_same_provider(self) -> None:
        for members in BUILT:
            with self.subTest(members=members):
                own = Anthropic(AnthropicConfig(**members))
                moved = Anthropic(RoomKitConfig(**members))
                self.assertEqual(said(moved), said(own))
                self.assertEqual(moved.catalog_entry(), own.catalog_entry())

    def test_every_member_of_roomkit_s_configuration_reaches_the_provider(self) -> None:
        # Each member stated otherwise than its default: none is left behind
        # when RoomKit's configuration is taken in place of this module's.
        stated = {
            "api_key": KEY,
            "model": "local-model",
            "max_tokens": 256,
            "timeout": 12.5,
            "connect_timeout": 2.5,
            "max_retries": 3,
            "base_url": "http://127.0.0.1:9",
            "extra_headers": {"X-Team": "a"},
            "enable_prompt_caching": False,
            "use_adaptive_thinking": True,
            "supports_custom_temperature": False,
        }
        self.assertEqual(set(stated), set(RoomKitConfig.model_fields))
        defaults = RoomKitConfig(api_key="", model="m").model_dump()
        for name, value in stated.items():
            self.assertNotEqual(value, defaults[name], name)
        crossed = []

        def recording(vendor: str, config_json: str, api_key: object) -> object:
            crossed.append((vendor, json.loads(config_json), api_key.get_secret_value()))
            return build(vendor, config_json, api_key)

        build = _native.provider
        # The switches a modern model decides cross as they were decided.
        decided = {"api_key": KEY, "model": MODERN}
        with mock.patch.object(_config._native, "provider", recording):
            for members in (stated, decided):
                Anthropic(RoomKitConfig(**members))
                Anthropic(AnthropicConfig(**members))
        self.assertEqual(crossed[0], crossed[1])
        vendor, members, key = crossed[0]
        self.assertEqual((vendor, key), ("anthropic", KEY))
        # The key crosses apart: the JSON holds it masked.
        self.assertEqual(members, {**stated, "api_key": "**********"})
        self.assertEqual(crossed[2], crossed[3])
        members = crossed[2][1]
        self.assertEqual(
            (members["use_adaptive_thinking"], members["supports_custom_temperature"]),
            (True, False),
        )

    def test_another_configuration_is_refused(self) -> None:
        class DerivedConfig(AnthropicConfig):
            region: str = "eu"

        class DerivedRoomKitConfig(RoomKitConfig):
            region: str = "eu"

        refused = (
            DerivedConfig(api_key=KEY, model="m"),
            DerivedRoomKitConfig(api_key=KEY, model="m"),
            {"api_key": KEY, "model": "m"},
        )
        for config in refused:
            with self.assertRaises(TypeError):
                Anthropic(config)

    def test_a_value_the_provider_s_reader_refuses_is_a_value_error(self) -> None:
        # RoomKit's configuration takes a time below zero; no clock counts it.
        config = AnthropicConfig(api_key=KEY, model="m", timeout=-1)
        with self.assertRaises(ValueError) as refused:
            Anthropic(config)
        self.assertNotIn(KEY, str(refused.exception))

    def test_a_provider_that_cannot_be_built_fails_as_roomkit_s_provider_error(self) -> None:
        config = AnthropicConfig(api_key=KEY, model="m", base_url="https://llm.example/?route=1")
        with self.assertRaises(ProviderError) as refused:
            Anthropic(config)
        self.assertTrue(refused.exception.before_request)
        self.assertEqual(refused.exception.provider, "anthropic")
        self.assertNotIn(KEY, str(refused.exception))

    def test_a_refused_configuration_leaves_no_secret_in_a_frame(self) -> None:
        for label, members in UNBUILT.items():
            for config in (AnthropicConfig, RoomKitConfig):
                with self.subTest(refused=label, config=config.__module__):
                    _, shown = self.refusal(config(**{**HELD, **members}))
                    for secret in SECRETS:
                        self.assertNotIn(secret, shown)

    def test_a_configuration_of_another_provider_leaves_none_either(self) -> None:
        class Derived(RoomKitConfig):
            region: str = "eu"

        error, shown = self.refusal(Derived(**HELD))
        self.assertIsInstance(error, TypeError)
        for secret in SECRETS:
            self.assertNotIn(secret, shown)


class Crossing:
    """A native provider's two entries of a generation, and what crossed to
    each: the request's JSON, and the key beside it."""

    def __init__(self, native: Any) -> None:
        self._native = native
        self.crossed: list[tuple[str, str | None]] = []

    def generate(self, request: str, key: str | None) -> Any:
        self.crossed.append((request, key))
        return self._native.generate(request, key)

    def stream(self, request: str, key: str | None) -> Any:
        self.crossed.append((request, key))
        return self._native.stream(request, key)


class CallTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        self.telemetry = MockTelemetryProvider()

    def provider(self, **members: object) -> Anthropic:
        provider = Anthropic(AnthropicConfig(**{**local_config(self.fake.port), **members}))
        # As RoomKit's AI channel hands a provider its telemetry.
        provider._telemetry = self.telemetry
        return provider

    def bodies(self) -> list[dict]:
        return [json.loads(seen.body) for seen in self.fake.seen]

    def first_tokens(self) -> list[dict]:
        return [m for m in self.telemetry.metrics if m["name"] == "roomkit.llm.ttfb_ms"]

    async def test_a_context_is_answered_with_roomkit_s_response(self) -> None:
        self.fake.answer(says("Hello ", "there."))
        context = AIContext(
            messages=[AIMessage(role="user", content="Hello")],
            system_prompt="You answer in one sentence.",
            # What a host left in the metadata has no JSON form, and does
            # not cross.
            metadata={"session": object()},
        )
        response = await self.provider().generate(context)
        self.assertIsInstance(response, AIResponse)
        self.assertEqual(
            (response.content, response.finish_reason, response.usage),
            ("Hello there.", "end_turn", {"input_tokens": 12, "output_tokens": 5}),
        )
        self.assertEqual((response.tasks, response.observations, response.tool_calls), ([], [], []))
        (seen,) = self.fake.seen
        self.assertEqual((seen.method, seen.target), ("POST", "/v1/messages"))
        self.assertEqual(seen.headers["x-api-key"], KEY)
        self.assertEqual(seen.headers["anthropic-version"], "2023-06-01")
        self.assertNotIn("authorization", seen.headers)
        (body,) = self.bodies()
        self.assertEqual([block["text"] for block in body["system"]], [context.system_prompt])
        self.assertEqual(blocks(body["messages"][0]), [{"type": "text", "text": "Hello"}])
        # A whole answer is asked as a stream too.
        self.assertIs(body["stream"], True)

    async def test_a_stream_gives_roomkit_s_events(self) -> None:
        self.fake.answer(
            answers(thinking("They greet me."), text("Hello.")),
            calls("lookup_order", '{"number": "A-1"}'),
            says("Hel", "lo."),
        )
        provider = self.provider()
        told = [event async for event in provider.generate_structured_stream(HELLO)]
        kinds = [StreamUsage, StreamThinkingDelta, StreamThinkingDelta, StreamTextDelta, StreamDone]
        self.assertEqual([type(event) for event in told], kinds)
        # What the message's start tells of its input, the reasoning, its
        # signature as a delta of its own, the text, then the end.
        self.assertEqual(told[0].usage, {"input_tokens": 12})
        self.assertEqual((told[1].thinking, told[1].signature), ("They greet me.", None))
        self.assertEqual((told[2].thinking, told[2].signature), ("", SIGNATURE))
        self.assertEqual((told[3].text, told[4].finish_reason), ("Hello.", "end_turn"))
        called = [event async for event in provider.generate_structured_stream(HELLO)]
        kinds = [StreamUsage, StreamToolCallDelta, StreamToolCallDelta, StreamToolCall, StreamDone]
        self.assertEqual([type(event) for event in called], kinds)
        self.assertEqual((called[3].id, called[3].arguments), ("toolu_1", {"number": "A-1"}))
        self.assertFalse(called[3].partial)
        self.assertEqual(called[4].finish_reason, "tool_use")
        self.assertEqual([piece async for piece in provider.generate_stream(HELLO)], ["Hel", "lo."])

    async def test_a_base_url_that_names_the_path_of_the_api_is_asked_at_that_path(self) -> None:
        self.fake.answer(says("Yes."))
        pasted = f"http://127.0.0.1:{self.fake.port}/v1/messages/"
        await self.provider(base_url=pasted).generate(HELLO)
        self.assertEqual(self.fake.seen[0].target, "/v1/messages")

    async def test_the_path_of_the_api_is_dropped_from_a_base_url_once(self) -> None:
        # RoomKit's configuration drops the path once, and its provider adds
        # it to what is left: a URL that names it twice is asked with it
        # twice.
        self.fake.answer(says("Yes."))
        twice = f"http://127.0.0.1:{self.fake.port}/v1/messages/v1/messages"
        for config in (AnthropicConfig, RoomKitConfig):
            kept = config(api_key=KEY, model="local-model", base_url=twice).base_url
            self.assertEqual(kept, f"http://127.0.0.1:{self.fake.port}/v1/messages")
        await self.provider(base_url=twice).generate(HELLO)
        self.assertEqual(self.fake.seen[0].target, "/v1/messages/v1/messages")

    async def test_the_key_a_request_brings_is_sent_for_that_request_alone(self) -> None:
        self.fake.answer(says("Yes."), says("Yes."), says("Yes."), says("Yes."))
        provider = self.provider()
        await provider.generate(HELLO)
        await provider.generate(lent())
        async for _ in provider.generate_structured_stream(lent()):
            pass
        # The next request carries the configuration's key again.
        await provider.generate(HELLO)
        keys = [seen.headers["x-api-key"] for seen in self.fake.seen]
        self.assertEqual(keys, [KEY, ONE_REQUEST_KEY, ONE_REQUEST_KEY, KEY])
        # A request carries one key, in that header and nowhere else.
        for seen in self.fake.seen:
            sent = " ".join(value for _, value in seen.lines) + seen.body.decode()
            self.assertEqual(sent.count(KEY) + sent.count(ONE_REQUEST_KEY), 1)

    async def test_neither_key_is_in_the_json_that_crosses(self) -> None:
        config = AnthropicConfig(**local_config(self.fake.port))
        self.assertNotIn(KEY, config.model_dump_json())
        self.fake.answer(says("Yes."), says("Yes."))
        provider = Anthropic(config)
        provider._native = crossing = Crossing(provider._native)
        await provider.generate(lent())
        async for _ in provider.generate_structured_stream(lent()):
            pass
        self.assertEqual(len(crossing.crossed), 2)
        for request, key in crossing.crossed:
            # The key of the request crosses apart from its JSON, which
            # holds nothing of a context's metadata.
            self.assertEqual(key, ONE_REQUEST_KEY)
            self.assertNotIn(ONE_REQUEST_KEY, request)
            self.assertNotIn(KEY, request)
            self.assertNotIn("metadata", json.loads(request))
        # And it reached the endpoint, in the header.
        self.assertEqual(
            [seen.headers["x-api-key"] for seen in self.fake.seen], [ONE_REQUEST_KEY] * 2
        )

    async def test_the_key_a_request_brings_is_in_no_failure_and_no_repr(self) -> None:
        # The server repeats what it was sent, as some do.
        words = f"invalid x-api-key {ONE_REQUEST_KEY}, which is not {KEY}"
        refusal = refuses(401, "authentication_error", words)
        self.fake.answer(refusal, refusal)
        provider, context = self.provider(), lent()
        with self.assertRaises(ProviderError) as whole:
            await provider.generate(context)
        with self.assertRaises(ProviderError) as streamed:
            await anext(provider.generate_structured_stream(context))
        self.assertEqual(
            [seen.headers["x-api-key"] for seen in self.fake.seen], [ONE_REQUEST_KEY] * 2
        )
        hidden = "invalid x-api-key [***], which is not [***]"
        for failed in (whole.exception, streamed.exception):
            self.assertEqual(failed.args, (hidden,))
            self.assertEqual(str(failed), f"anthropic (401): {hidden}")
            self.assertEqual((failed.status_code, failed.retryable), (401, False))
            for secret in (ONE_REQUEST_KEY, KEY):
                self.assertNotIn(secret, f"{failed} {failed!r} {failed.args} {vars(failed)}")
        # Nor is it in what the provider and the context show of themselves.
        self.assertNotIn(ONE_REQUEST_KEY, f"{provider!r} {context!r} {context.metadata!r}")

    async def test_a_failing_call_leaves_no_secret_in_a_frame(self) -> None:
        # A secret in each place a provider holds one: its key, a header's
        # value, the password of its base URL, the key of the request.
        provider = self.provider(
            base_url=f"http://team:{PASSWORD}@127.0.0.1:{self.fake.port}",
            extra_headers={"X-Team-Token": HEADER},
        )

        async def failures(context: AIContext) -> list[str]:
            """What the traceback of a whole answer's failure writes, and of
            a stream's: read while its frames still hold their locals."""
            shown = []
            for asked in (
                provider.generate(context),
                anext(provider.generate_structured_stream(context)),
            ):
                try:
                    await asked
                except ProviderError as failed:
                    shown.append(written(failed))
            self.assertEqual(len(shown), 2, "a call did not fail")
            return shown

        words = f"Bad key {KEY}, header {HEADER}, password {PASSWORD}, key {ONE_REQUEST_KEY}"
        refusal = refuses(401, "authentication_error", words)
        unwritten = AIContext(
            messages=[AIMessage(role="user", content="Hello", metadata={"handle": object()})],
            metadata={"api_key": ONE_REQUEST_KEY},
        )
        cases = {
            "a refusal that repeats the secrets": ([refusal, refusal], lent()),
            "a connection that is refused": ([Refused(), Refused()], lent()),
            "a request the contract refuses": ([], lent(role="developer")),
            "a context with no JSON form": ([], unwritten),
        }
        for label, (replies, context) in cases.items():
            with self.subTest(failing=label):
                self.fake.answer(*replies)
                for shown in await failures(context):
                    for secret in SECRETS:
                        self.assertNotIn(secret, shown)
                # The request that was sent did carry them.
                for seen in self.fake.seen:
                    self.assertEqual(seen.headers["x-api-key"], ONE_REQUEST_KEY)
                    self.assertEqual(seen.headers["x-team-token"], HEADER)
                    self.assertTrue(seen.headers["authorization"].startswith("Basic "))

    async def test_reasoning_and_redacted_blocks_go_back_as_they_came_after_a_round_of_calls(
        self,
    ) -> None:
        # A first answer that reasons in two signed blocks around a redacted
        # one, says a word and calls a tool; then the answer to its result.
        self.fake.answer(
            answers(
                thinking("The order ", "is A-1."),
                redacted(),
                thinking("Then its status.", signature=OTHER_SIGNATURE),
                text("Let me look."),
                call("lookup_order", '{"number": "A-1"}'),
                stop="tool_use",
            ),
            says("It left this morning."),
        )
        # No mark of the prompt cache: a block is compared member for member.
        provider = self.provider(enable_prompt_caching=False)
        asked = AIContext(
            messages=[AIMessage(role="user", content="Where is my order A-1?")],
            tools=[ORDER],
            enable_thinking=True,
        )
        first = await provider.generate(asked)
        self.assertEqual(
            first.thinking_parts,
            [
                AIThinkingPart(thinking="The order is A-1.", signature=SIGNATURE),
                AIThinkingPart(thinking="", redacted=REDACTED),
                AIThinkingPart(thinking="Then its status.", signature=OTHER_SIGNATURE),
            ],
        )
        (called,) = first.tool_calls
        # The round as RoomKit replays one: its reasoning, its text and its
        # call, then what the tool answered.
        replayed = [
            AIMessage(
                role="assistant",
                content=round_parts(thinking_parts_of(first), first.content, first.tool_calls),
            ),
            AIMessage(
                role="tool",
                content=[
                    AIToolResultPart(
                        tool_call_id=called.id, name=called.name, result="Left this morning."
                    )
                ],
            ),
        ]
        again = asked.model_copy(update={"messages": [*asked.messages, *replayed]})
        self.assertEqual((await provider.generate(again)).content, "It left this morning.")
        _, assistant, results = self.bodies()[1]["messages"]
        # Each block of reasoning is Anthropic's own again, with the
        # signature or the data that came with it.
        self.assertEqual(
            assistant,
            {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "The order is A-1.", "signature": SIGNATURE},
                    {"type": "redacted_thinking", "data": REDACTED},
                    {
                        "type": "thinking",
                        "thinking": "Then its status.",
                        "signature": OTHER_SIGNATURE,
                    },
                    {"type": "text", "text": "Let me look."},
                    {
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "lookup_order",
                        "input": {"number": "A-1"},
                    },
                ],
            },
        )
        result = {"type": "tool_result", "tool_use_id": "toolu_1", "content": "Left this morning."}
        self.assertEqual(results, {"role": "user", "content": [result]})

    async def test_a_tool_held_unseen_is_declared_so_and_a_result_names_the_tools_it_shows(
        self,
    ) -> None:
        find = AITool(
            name="find_tools",
            description="Finds the tools that fit a task.",
            parameters={"type": "object", "properties": {"query": {"type": "string"}}},
        )
        found = AIToolResultPart(
            tool_call_id="toolu_1",
            name="find_tools",
            result="Found 1 tool: lookup_order.",
            references=["lookup_order"],
        )
        search = AIToolCallPart(id="toolu_1", name="find_tools", arguments={"query": "orders"})
        context = AIContext(
            messages=[
                AIMessage(role="user", content="Where is my order A-1?"),
                AIMessage(role="assistant", content=[search]),
                AIMessage(role="tool", content=[found]),
            ],
            tools=[find, ORDER.model_copy(update={"defer_loading": True})],
        )
        self.fake.answer(calls("lookup_order", '{"number": "A-1"}'))
        response = await self.provider().generate(context)
        self.assertEqual([called.name for called in response.tool_calls], ["lookup_order"])
        (body,) = self.bodies()
        shown, unseen = body["tools"]
        self.assertEqual((shown["name"], unseen["name"]), ("find_tools", "lookup_order"))
        self.assertNotIn("defer_loading", shown)
        self.assertIs(unseen["defer_loading"], True)
        # The result carries the references alone, as the API requires;
        # what the tool returned follows, named after its call and set apart.
        self.assertEqual(
            blocks(body["messages"][2]),
            [
                {
                    "type": "tool_result",
                    "tool_use_id": "toolu_1",
                    "content": [{"type": "tool_reference", "tool_name": "lookup_order"}],
                },
                {
                    "type": "text",
                    "text": (
                        "[Result of find_tools]\n<tool_result>\n"
                        "Found 1 tool: lookup_order.\n</tool_result>"
                    ),
                },
            ],
        )

    async def test_a_response_schema_is_asked_and_checked_with_and_without_tools(self) -> None:
        alone = HELLO.model_copy(update={"response_schema": SCHEMA})
        with_tools = alone.model_copy(update={"tools": [ORDER]})
        self.fake.answer(
            says('{"city": ', '"Paris"}'),
            calls("lookup_order", '{"number": "A-1"}', text("Let me look.")),
            says('{"city": "Paris"}'),
        )
        provider = self.provider()
        self.assertEqual(json.loads((await provider.generate(alone)).content), {"city": "Paris"})
        # A round that calls a tool is no answer: its text is not checked.
        called = await provider.generate(with_tools)
        self.assertEqual(
            (called.content, [made.name for made in called.tool_calls]),
            ("Let me look.", ["lookup_order"]),
        )
        answered = await provider.generate(with_tools)
        self.assertEqual(json.loads(answered.content), {"city": "Paris"})
        first, second, third = self.bodies()
        asked = {"format": {"type": "json_schema", "schema": SCHEMA}}
        self.assertEqual([body["output_config"] for body in (first, second, third)], [asked] * 3)
        self.assertNotIn("tools", first)
        for body in (second, third):
            self.assertEqual([tool["name"] for tool in body["tools"]], ["lookup_order"])
        # An answer that is no document of the schema fails, tools or not.
        for context in (alone, with_tools):
            self.fake.answer(says("It is Paris."))
            with self.assertRaises(ResponseSchemaError) as refused:
                await provider.generate(context)
            self.assertEqual(refused.exception.reason, "invalid_json")

    async def test_a_refusal_is_a_finish_reason_and_fails_an_answer_under_a_schema(self) -> None:
        self.fake.answer(declines(), declines(), declines())
        provider = self.provider()
        declined = await provider.generate(HELLO)
        self.assertEqual(
            (declined.content, declined.finish_reason, declined.metadata["refusal"]),
            ("", "refusal", "cyber"),
        )
        asked = HELLO.model_copy(update={"response_schema": SCHEMA})
        with self.assertRaises(ResponseSchemaError) as whole:
            await provider.generate(asked)
        given = []
        with self.assertRaises(ResponseSchemaError) as streamed:
            async for event in provider.generate_structured_stream(asked):
                given.append(type(event))
        # The failure takes the place of the stream's done event.
        self.assertEqual(given, [StreamUsage])
        for failed in (whole.exception, streamed.exception):
            self.assertIs(type(failed), ResponseSchemaError)
            self.assertEqual(str(failed), "anthropic: the model declined to answer: refusal")
            self.assertEqual(
                (failed.reason, failed.provider, failed.retryable, failed.before_request),
                ("refusal", "anthropic", False, False),
            )

    async def test_the_switches_of_a_configuration_reach_the_wire(self) -> None:
        warm = HELLO.model_copy(update={"temperature": 0.2})
        thinks = HELLO.model_copy(update={"thinking_budget": 2048, "max_tokens": 4096})
        # Behind a base URL no model is profiled: a switch is what the
        # configuration states, or its default. What is stated, then the
        # temperature that is sent and how a thinking budget is asked.
        cases = [
            ({}, 0.2, BUDGET),
            ({"supports_custom_temperature": False}, None, BUDGET),
            ({"use_adaptive_thinking": True}, 0.2, ADAPTIVE),
        ]
        for stated, temperature, asked in cases:
            with self.subTest(stated=stated):
                self.fake.answer(says("Yes."), says("Yes."))
                provider = self.provider(model=MODERN, **stated)
                await provider.generate(warm)
                await provider.generate(thinks)
                plain, thought = self.bodies()
                self.assertEqual(plain.get("temperature"), temperature)
                self.assertNotIn("thinking", plain)
                self.assertEqual(thought["thinking"], asked)
                # A turn that thinks sends no temperature.
                self.assertNotIn("temperature", thought)

    async def test_the_models_an_endpoint_lists_are_roomkit_s(self) -> None:
        listed = models((MODERN, "Claude Opus 5.5"), ("claude-next-1", "Claude Next 1"))
        self.fake.answer(Whole(200, JSON, listed), refuses(401, "authentication_error", "no"))
        provider = self.provider()
        found = await provider.list_models()
        self.assertTrue(all(isinstance(model, ModelInfo) for model in found))
        self.assertEqual([model.id for model in found], [MODERN, "claude-next-1"])
        # The name is the endpoint's; the window is the catalogue's, and a
        # model the catalogue does not know passes through.
        self.assertEqual(
            [(model.display_name, model.context_window) for model in found],
            [("Claude Opus 5.5", 1000000), ("Claude Next 1", None)],
        )
        (seen,) = self.fake.seen
        self.assertEqual((seen.method, seen.target), ("GET", "/v1/models?limit=1000"))
        self.assertEqual(seen.headers["x-api-key"], KEY)
        # A listing fails as a generation does.
        with self.assertRaises(ProviderError) as refused:
            await provider.list_models()
        failed = refused.exception
        self.assertEqual((failed.provider, failed.status_code), ("anthropic", 401))

    async def test_a_failure_names_anthropic_and_carries_roomkit_s_attributes(self) -> None:
        # A status, the type and the words of its error; then whether a new
        # request may succeed, and whether the context was too long.
        cases = [
            (401, "authentication_error", "invalid x-api-key", False, None),
            (429, "rate_limit_error", "This request would exceed the rate limit.", True, None),
            (529, "overloaded_error", "Overloaded", True, None),
            (
                400,
                "invalid_request_error",
                "prompt is too long: 210000 tokens > 200000 maximum",
                False,
                True,
            ),
        ]
        provider = self.provider()
        for status, kind, words, retryable, overflow in cases:
            with self.subTest(status=status):
                self.fake.answer(refuses(status, kind, words), refuses(status, kind, words))
                with self.assertRaises(ProviderError) as whole:
                    await provider.generate(HELLO)
                with self.assertRaises(ProviderError) as streamed:
                    await anext(provider.generate_structured_stream(HELLO))
                told = words
                for failed in (whole.exception, streamed.exception):
                    self.assertIs(type(failed), ProviderError)
                    self.assertEqual(failed.args, (told,))
                    self.assertEqual(str(failed), f"anthropic ({status}): {told}")
                    self.assertEqual(
                        (failed.retryable, failed.provider, failed.status_code),
                        (retryable, "anthropic", status),
                    )
                    self.assertIs(failed.context_overflow, overflow)
                    self.assertFalse(failed.before_request)

    async def test_a_context_the_contract_refuses_fails_before_the_request(self) -> None:
        # RoomKit takes any role and its own provider sends it; the contract
        # knows four.
        context = AIContext(messages=[AIMessage(role="developer", content="Be brief.")])
        provider = self.provider()
        with self.assertRaises(ProviderError) as whole:
            await provider.generate(context)
        with self.assertRaises(ProviderError) as streamed:
            await anext(provider.generate_structured_stream(context))
        for refused in (whole.exception, streamed.exception):
            self.assertTrue(refused.before_request)
            self.assertFalse(refused.retryable)
            self.assertEqual(refused.provider, "anthropic")
            self.assertTrue(str(refused).startswith("anthropic: the request is refused: "))
        self.assertEqual(self.fake.seen, [])

    async def test_the_first_token_of_a_whole_answer_is_recorded_once(self) -> None:
        self.fake.answer(says("Yes."), refuses(401, "authentication_error", "invalid x-api-key"))
        provider = self.provider()
        asked = time.monotonic()
        await provider.generate(HELLO)
        (metric,) = self.first_tokens()
        self.assertEqual(metric["unit"], "ms")
        # Its value: the milliseconds the answer took to come.
        self.assertIsInstance(metric["value"], float)
        self.assertTrue(0 < metric["value"] <= (time.monotonic() - asked) * 1000, metric)
        # Its label is the name RoomKit's own provider goes by.
        self.assertEqual(
            metric["attributes"], {"provider": "AnthropicAIProvider", "model": "local-model"}
        )
        # A request that failed recorded none.
        with self.assertRaises(ProviderError):
            await provider.generate(HELLO)
        self.assertEqual(len(self.first_tokens()), 1)

    async def test_a_whole_answer_of_calls_alone_records_no_first_token(self) -> None:
        order = ("lookup_order", '{"number": "A-1"}')
        cases = {
            "a call alone": calls(*order),
            "a redacted block then a call": calls(*order, redacted()),
            "a signature alone then a call": calls(*order, thinking()),
        }
        for label, reply in cases.items():
            with self.subTest(answer=label):
                self.fake.answer(reply)
                called = await self.provider().generate(HELLO)
                self.assertEqual([made.name for made in called.tool_calls], ["lookup_order"])
                self.assertEqual(self.first_tokens(), [])

    async def test_the_first_token_of_a_stream_is_its_first_text_or_reasoning(self) -> None:
        order = call("lookup_order", '{"number": "A-1"}')
        called = [StreamToolCallDelta, StreamToolCallDelta, StreamToolCall, StreamDone]
        # A stream, the events that come before its first token is recorded,
        # and the event it is recorded at: none where the stream holds no
        # text and no reasoning.
        cases = [
            ("text", says("Hel", "lo."), [StreamUsage], StreamTextDelta),
            (
                "reasoning then text",
                answers(thinking("They greet me."), text("Hello.")),
                [StreamUsage],
                StreamThinkingDelta,
            ),
            (
                "a redacted block then text",
                answers(redacted(), text("Hello.")),
                [StreamUsage, StreamThinkingDelta],
                StreamTextDelta,
            ),
            (
                "a signature alone then a call",
                answers(thinking(), order, stop="tool_use"),
                [StreamUsage, StreamThinkingDelta, *called],
                None,
            ),
        ]
        for label, reply, before, first in cases:
            with self.subTest(stream=label):
                self.telemetry.metrics.clear()
                self.fake.answer(reply)
                came: list[tuple[type, int]] = []
                async for event in self.provider().generate_structured_stream(HELLO):
                    came.append((type(event), len(self.first_tokens())))
                unrecorded = [kind for kind, recorded in came if not recorded]
                self.assertEqual(unrecorded, before)
                # It was recorded when that event came, and once.
                at = came[len(before)][0] if len(before) < len(came) else None
                self.assertIs(at, first)
                self.assertEqual(came[-1][1], 0 if first is None else 1)
        self.telemetry.metrics.clear()
        self.fake.answer(says("Hel", "lo."))
        async for _ in self.provider().generate_stream(HELLO):
            pass
        (metric,) = self.first_tokens()
        self.assertEqual(metric["attributes"]["provider"], "AnthropicAIProvider")

    async def test_cancelling_a_call_or_closing_a_stream_closes_its_connection(self) -> None:
        self.fake.answer(Silent(), opens_and_waits("Hel"), opens_and_waits("Hel"))
        provider = self.provider()
        # A call whose answer never starts, then one whose answer stops
        # under way: a whole answer is read as a stream.
        for _ in range(2):
            asking = asyncio.create_task(provider.generate(HELLO))
            await self.fake.silent()
            asking.cancel()
            await self.fake.closed()
            with self.assertRaises(asyncio.CancelledError):
                await asking
        stream = provider.generate_structured_stream(HELLO)
        self.assertIsInstance(await anext(stream), StreamUsage)
        self.assertEqual((await anext(stream)).text, "Hel")
        await stream.aclose()
        await self.fake.closed()
        self.assertEqual(len(self.fake.seen), 3)
        # Closing the provider returns nothing, a second time too.
        self.assertIsNone(await provider.close())
        self.assertIsNone(await provider.close())


@unittest.skipUnless(OPENSSL, NO_OPENSSL)
@unittest.skipIf(routed(), NO_NAMESPACE)
class OwnEndpointTests(unittest.IsolatedAsyncioTestCase):
    """Anthropic's own endpoint, on this machine: a provider that names no
    base URL is built behind a local proxy, whose tunnel leads to a local
    server. That server shows a certificate for the endpoint's name, signed
    by an authority made for the test."""

    async def asyncSetUp(self) -> None:
        self.authority = Authority()
        self.addCleanup(self.authority.close)
        self.endpoint = await Fake(self.authority.serving("api.anthropic.com")).start()
        self.addAsyncCleanup(self.endpoint.stop)
        self.proxy = await Fake().start()
        self.addAsyncCleanup(self.proxy.stop)

    async def sent(self, contexts: list[AIContext], **members: Any) -> list[dict]:
        """The bodies Anthropic's own endpoint is sent for `contexts` by a
        provider of `members`, which name no base URL."""
        self.proxy.answer(*(Tunnel(self.endpoint.port) for _ in contexts))
        self.endpoint.answer(*(says("Yes.") for _ in contexts))
        variables = {
            "HTTPS_PROXY": f"http://127.0.0.1:{self.proxy.port}",
            "SSL_CERT_FILE": str(self.authority.pem),
        }
        with environment(**variables):
            provider = Anthropic(AnthropicConfig(api_key=KEY, **members))
            for context in contexts:
                self.assertEqual((await provider.generate(context)).content, "Yes.")
            await provider.close()
        tunnels = {(seen.method, seen.target) for seen in self.proxy.seen}
        self.assertEqual(tunnels, {("CONNECT", "api.anthropic.com:443")})
        for seen in self.endpoint.seen:
            self.assertEqual((seen.method, seen.target), ("POST", "/v1/messages"))
            self.assertEqual(seen.headers["host"], "api.anthropic.com")
            self.assertEqual(seen.headers["x-api-key"], KEY)
        return [json.loads(seen.body) for seen in self.endpoint.seen]

    async def test_what_a_model_decides_on_anthropic_s_own_endpoint_reaches_the_wire(self) -> None:
        warm = HELLO.model_copy(update={"temperature": 0.2})
        thinks = HELLO.model_copy(update={"thinking_budget": 2048, "max_tokens": 4096})
        stated = {"use_adaptive_thinking": False, "supports_custom_temperature": True}
        # A model, what is stated beside it, then the temperature that is
        # sent and how a thinking budget is asked.
        cases = [
            (MODERN, {}, None, ADAPTIVE),
            (OLD, {}, 0.2, BUDGET),
            (MODERN, stated, 0.2, BUDGET),
        ]
        for model, members, temperature, asked in cases:
            with self.subTest(model=model, stated=members):
                plain, thought = await self.sent([warm, thinks], model=model, **members)
                self.assertEqual(plain.get("temperature"), temperature)
                self.assertEqual(thought["thinking"], asked)
                self.assertNotIn("temperature", thought)


# What the child of the fork test runs: a provider that answered once, then
# a forked child to which the native module is a stand-in that fails as soon
# as anything of it is read.
FORKED = """
import asyncio
import multiprocessing
import sys
import warnings

sys.path.insert(0, sys.argv[1])
from fake_anthropic import local_config, says
from fake_endpoint import ThreadedFake

import roomkit_providers._process
import roomkit_providers.llm._config
import roomkit_providers.llm._provider
from roomkit.providers.ai import AIContext, AIMessage
from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
STATED = {"use_adaptive_thinking": False, "supports_custom_temperature": True}
# Forking a process with threads is the scenario under test.
warnings.filterwarnings("ignore", category=DeprecationWarning)
fake = ThreadedFake()
config = AnthropicConfig(**local_config(fake.port))
provider = Anthropic(config)


class Entered:
    # The native module, to a child that must not enter it.
    def __getattr__(self, name):
        raise AssertionError(f"the child entered the native module: {name}")


async def ask():
    return (await provider.generate(HELLO)).content


async def reads(stream):
    return [event async for event in stream]


def child(queue):
    for module in (
        roomkit_providers._process,
        roomkit_providers.llm._config,
        roomkit_providers.llm._provider,
    ):
        module._native = Entered()
    provider._native = Entered()
    refused = []
    for attempt in (
        lambda: asyncio.run(ask()),
        lambda: asyncio.run(provider.list_models()),
        lambda: asyncio.run(reads(provider.generate_structured_stream(HELLO))),
        lambda: asyncio.run(reads(provider.generate_stream(HELLO))),
        lambda: Anthropic(config),
        # A configuration whose model decides its switches, and one that
        # states them both.
        lambda: AnthropicConfig(api_key="placeholder-not-a-key", model="claude-opus-5-5"),
        lambda: AnthropicConfig(**local_config(fake.port), **STATED),
    ):
        try:
            attempt()
        except RuntimeError as error:
            refused.append("forked" in str(error))
        except BaseException as error:
            refused.append(repr(error))
        else:
            refused.append("not refused")
    # What a provider says of itself was read when it was built, and its
    # catalogue when its class was defined; closing it does nothing.
    told = (provider.name, provider.model_name, provider.supports_vision)
    listed = [model.id for model in Anthropic.available_models()]
    closed = asyncio.run(provider.close())
    queue.put((refused, told, listed, closed))


fake.answer(says("Yes."))
assert asyncio.run(ask()) == "Yes."
context = multiprocessing.get_context("fork")
queue = context.Queue()
process = context.Process(target=child, args=(queue,))
process.start()
refused, told, listed, closed = queue.get(timeout=30)
process.join(30)
assert refused == [True] * 7, refused
assert told == ("AnthropicAIProvider", "local-model", False), told
assert listed and listed == [model.id for model in Anthropic.available_models()], listed
assert closed is None
assert process.exitcode == 0, process.exitcode
# The parent keeps its provider.
fake.answer(says("Yes."))
assert asyncio.run(ask()) == "Yes."
fake.stop()
print("refused in Python, and the catalogue answered")
"""


@unittest.skipUnless(hasattr(os, "fork"), "os.fork")
class ForkTests(unittest.TestCase):
    def test_a_forked_child_is_refused_in_python_and_its_catalogue_still_answers(self) -> None:
        # The scenario runs in an interpreter of its own, which writes no
        # bytecode beside the tests.
        result = subprocess.run(
            [sys.executable, "-X", "faulthandler", "-I", "-B", "-c", FORKED, str(HERE)],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Fatal Python error", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn("refused in Python, and the catalogue answered", result.stdout)


class Agent:
    """A room with a user on a WebSocket channel and an AI channel over the
    provider, and what the user's connection was delivered."""

    def __init__(self, fake: Fake, *, streaming: bool, telemetry: Any = None) -> None:
        self.kit = RoomKit(telemetry=telemetry)
        self.provider = Anthropic(AnthropicConfig(**local_config(fake.port)))
        self.asked: list[tuple[str, dict]] = []
        self.rows: list[str] = []
        self.stream: list[object] = []
        self.calls: list[Any] = []
        self.streaming = streaming

    async def handle(self, name: str, arguments: dict) -> str:
        self.asked.append((name, arguments))
        return json.dumps({"number": arguments["number"], "status": "left this morning"})

    async def open(self) -> None:
        kit = self.kit
        assistant = AIChannel(
            "assistant",
            provider=self.provider,
            system_prompt="You answer in one sentence.",
            tools=[ORDER],
            tool_handler=self.handle,
        )
        user = WebSocketChannel("user")

        async def row(_connection: str, event: Any) -> None:
            if isinstance(event.content, TextContent):
                self.rows.append(event.content.body)

        async def streamed(_connection: str, message: object) -> None:
            self.stream.append(message)

        @kit.hook(HookTrigger.ON_TOOL_CALL, execution=HookExecution.ASYNC)
        async def observe(event: Any, _context: Any) -> None:
            self.calls.append(event)

        stream_send = streamed if self.streaming else None
        user.register_connection("console", row, room_id="orders", stream_send_fn=stream_send)
        kit.register_channel(user)
        kit.register_channel(assistant)
        await kit.create_room(room_id="orders")
        await kit.attach_channel("orders", "user")
        await kit.attach_channel("orders", "assistant", category=ChannelCategory.INTELLIGENCE)

    async def says(self, said_: str) -> None:
        """The user says it: the turn it starts is over when this returns."""
        message = InboundMessage(
            channel_id="user", sender_id="customer", content=TextContent(body=said_)
        )
        await self.kit.process_inbound(message, room_id="orders")


class AgentTests(unittest.IsolatedAsyncioTestCase):
    """A RoomKit agent over the provider. Everything is observed through
    RoomKit: what its channels deliver, what its tool handler is asked, what
    its telemetry records; and on the fake, what each round was sent."""

    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)

    async def agent(self, **settings: Any) -> Agent:
        agent = Agent(self.fake, **settings)
        self.addAsyncCleanup(agent.kit.close)
        await agent.open()
        return agent

    def bodies(self) -> list[dict]:
        return [json.loads(seen.body) for seen in self.fake.seen]

    async def test_an_agent_answers_a_message(self) -> None:
        self.fake.answer(says("We open ", "at nine."))
        agent = await self.agent(streaming=False)
        async with asyncio.timeout(DEADLINE):
            await agent.says("When do you open?")
        # The room delivered the answer to the user's connection, whole.
        self.assertEqual(agent.rows, ["We open at nine."])
        (asked,) = self.bodies()
        self.assertIn("You answer in one sentence.", [block["text"] for block in asked["system"]])
        last = asked["messages"][-1]
        self.assertEqual(last["role"], "user")
        self.assertEqual(blocks(last), [{"type": "text", "text": "When do you open?"}])
        # The channel's tools, RoomKit's own among them, were declared.
        self.assertIn("lookup_order", [tool["name"] for tool in asked["tools"]])
        self.assertIs(asked["stream"], True)

    async def test_an_agent_calls_a_tool_and_answers_with_its_result(self) -> None:
        # The model reasons, in a signed block and a redacted one, before it
        # calls the tool.
        self.fake.answer(
            calls(
                "lookup_order", '{"number": "A-1"}', thinking("The order ", "is A-1."), redacted()
            ),
            says("Your order ", "left this morning."),
        )
        agent = await self.agent(streaming=False)
        async with asyncio.timeout(DEADLINE):
            await agent.says("Where is my order A-1?")
        # RoomKit ran the call the model made, and told its hook.
        self.assertEqual(agent.asked, [("lookup_order", {"number": "A-1"})])
        self.assertEqual(
            [(made.name, made.is_error) for made in agent.calls], [("lookup_order", False)]
        )
        # The second round showed the model its round and what the tool
        # answered: the reasoning as it came, with its signature and its
        # data, then the call.
        first, second = self.bodies()
        self.assertEqual(len(second["messages"]), len(first["messages"]) + 2)
        called, answered = second["messages"][-2:]
        self.assertEqual(called["role"], "assistant")
        self.assertEqual(
            blocks(called),
            [
                {"type": "thinking", "thinking": "The order is A-1.", "signature": SIGNATURE},
                {"type": "redacted_thinking", "data": REDACTED},
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "lookup_order",
                    "input": {"number": "A-1"},
                },
            ],
        )
        self.assertEqual(answered["role"], "user")
        (result,) = blocks(answered)
        self.assertEqual((result["type"], result["tool_use_id"]), ("tool_result", "toolu_1"))
        self.assertEqual(json.loads(result["content"])["status"], "left this morning")
        self.assertEqual(agent.rows, ["Your order left this morning."])

    async def test_an_agent_streams_its_answer_to_a_channel_that_takes_streams(self) -> None:
        self.fake.answer(says("We ", "open ", "at nine."))
        agent = await self.agent(streaming=True)
        async with asyncio.timeout(DEADLINE):
            await agent.says("When do you open?")
        kinds = [type(message) for message in agent.stream]
        self.assertIs(kinds[0], StreamStart)
        self.assertIs(kinds[-1], StreamEnd)
        chunks = [message for message in agent.stream if isinstance(message, StreamChunk)]
        self.assertEqual("".join(chunk.delta for chunk in chunks), "We open at nine.")
        self.assertEqual(chunks[-1].text, "We open at nine.")

    async def test_a_turn_records_its_first_token_once_a_generation_that_wrote(self) -> None:
        telemetry = MockTelemetryProvider()
        self.fake.answer(
            calls("lookup_order", '{"number": "A-1"}', redacted()),
            says("Your order ", "left ", "this morning."),
        )
        agent = await self.agent(streaming=True, telemetry=telemetry)
        async with asyncio.timeout(DEADLINE):
            await agent.says("Where is my order A-1?")
        # Two generations: the first wrote a redacted block and a call,
        # which is no token of an answer; the second wrote text.
        recorded = [
            metric for metric in telemetry.metrics if metric["name"] == "roomkit.llm.ttfb_ms"
        ]
        self.assertEqual(len(recorded), 1, telemetry.metrics)
        (metric,) = recorded
        self.assertEqual(metric["unit"], "ms")
        self.assertEqual(
            metric["attributes"], {"provider": "AnthropicAIProvider", "model": "local-model"}
        )
        self.assertGreaterEqual(metric["value"], 0)


if __name__ == "__main__":
    unittest.main()
