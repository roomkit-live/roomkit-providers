"""OpenAI's provider as a RoomKit program holds it: its configuration, what
it says of itself, and its calls against a local fake of the endpoint, in
RoomKit's models and RoomKit's exceptions."""

from __future__ import annotations

import asyncio
import json
import tomllib
import unittest
from pathlib import Path

import roomkit_providers
from fake_endpoint import (
    EVENT_STREAM,
    JSON,
    Fake,
    Reads,
    Silent,
    Whole,
    call_events,
    completion,
    events,
    local_config,
)
from pydantic import ValidationError
from roomkit.providers.ai import (
    AIContext,
    AIMessage,
    AIProvider,
    AIResponse,
    ModelInfo,
    ProviderError,
    ResponseSchemaError,
    StreamDone,
    StreamTextDelta,
    StreamThinkingDelta,
    StreamToolCall,
    StreamToolCallDelta,
)
from roomkit.providers.openai import OpenAIAIProvider as RoomKitProvider
from roomkit.providers.openai import OpenAIConfig as RoomKitConfig
from roomkit.telemetry import MockTelemetryProvider
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

ROOT = Path(__file__).resolve().parents[2]
KEY = "placeholder-not-a-key"
HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])

# Configurations RoomKit's own class takes, as keyword arguments: a model
# profiled on OpenAI's own endpoint, however that endpoint is written, a
# switch stated, another server, values pydantic converts, an argument
# neither class knows.
TAKEN = [
    {"api_key": KEY, "model": "gpt-4o"},
    {"api_key": KEY, "model": "gpt-5.1"},
    {"api_key": KEY, "model": "o3"},
    {"api_key": KEY, "model": "gpt-4.1-mini"},
    {"api_key": KEY, "model": "gpt-5.1", "use_max_completion_tokens": False},
    {"api_key": KEY, "model": "o3", "supports_custom_temperature": True},
    {"api_key": KEY, "model": "gpt-5.1", "base_url": "http://localhost:1/v1"},
    {"api_key": KEY, "model": "gpt-5.1", "base_url": "https://api.openai.com:443/v1"},
    {"api_key": KEY, "model": "gpt-5.1", "base_url": "https://api.openai.com/v2"},
    {"api_key": KEY, "model": "gpt-5.1", "base_url": "https://u:p@api.openai.com/v1"},
    {"api_key": "", "model": "m", "timeout": "45", "max_tokens": "256", "max_retries": 2.0},
    {"api_key": KEY, "model": "m", "max_tokens": None, "include_stream_usage": 0},
    {"api_key": KEY, "model": "m", "default_headers": {"x-team": "a"}, "extra_body": {"top_k": 4}},
    {"api_key": KEY, "model": "m", "reasoning_effort": "low", "thinking_field": "reasoning"},
    {"api_key": KEY, "model": "m", "supports_response_schema": False, "region": "eu"},
]
# OpenAI's own endpoint written with spaces around it: read as RoomKit
# reads it, and no provider is built with it.
SPACED = {"api_key": KEY, "model": "gpt-5.1", "base_url": " HTTPS://Api.OpenAI.com/v1/ "}
# Configurations RoomKit's own class refuses.
REFUSED = [
    {},
    {"api_key": KEY},
    {"api_key": None, "model": "m"},
    {"api_key": KEY, "model": "m", "timeout": "soon"},
    {"api_key": KEY, "model": "m", "max_tokens": 1.5},
    {"api_key": KEY, "model": "m", "use_max_completion_tokens": None},
    {"api_key": KEY, "model": "m", "default_headers": {"x-team": 1}},
]


def reasoning_then_text() -> list[bytes]:
    """The reads of a streamed answer that reasons in a field, then says
    "Hello.", then ends."""
    first, *rest = events("Hello.")
    thought = {"index": 0, "delta": {"reasoning_content": "They greet me."}}
    chunk = {"id": "chatcmpl-1", "model": "local-model", "choices": [thought]}
    return [f"data: {json.dumps(chunk)}\n\n".encode(), first, *rest]


class ConfigTests(unittest.TestCase):
    def test_the_fields_and_the_defaults_are_those_of_roomkit_s_configuration(self) -> None:
        def fields(config: type) -> dict:
            return {
                name: (field.annotation, field.default, field.is_required())
                for name, field in config.model_fields.items()
            }

        self.assertEqual(fields(OpenAIConfig), fields(RoomKitConfig))

    def test_a_configuration_is_read_as_roomkit_s_reads_it(self) -> None:
        for members in [*TAKEN, SPACED]:
            with self.subTest(members=members):
                own, roomkit_s = OpenAIConfig(**members), RoomKitConfig(**members)
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

        for members in REFUSED:
            with self.subTest(members=members):
                self.assertEqual(refusal(OpenAIConfig, members), refusal(RoomKitConfig, members))

    def test_a_positional_argument_is_refused(self) -> None:
        with self.assertRaises(TypeError):
            OpenAIConfig(KEY, "gpt-4o")

    def test_repr_shows_no_key_no_header_value_and_no_credentials_of_the_url(self) -> None:
        config = OpenAIConfig(
            api_key=KEY,
            model="gpt-4o",
            base_url="https://team:placeholder-password@llm.example/v1",
            default_headers={"Authorization": "Basic placeholder-header"},
        )
        for shown in (repr(config), str(config)):
            self.assertNotIn(KEY, shown)
            self.assertNotIn("placeholder-password", shown)
            self.assertNotIn("team", shown)
            self.assertNotIn("placeholder-header", shown)
            self.assertIn("llm.example/v1", shown)
            self.assertIn("Authorization", shown)
        plain = repr(OpenAIConfig(api_key=KEY, model="m", base_url="http://localhost:1/v1"))
        self.assertIn("base_url='http://localhost:1/v1'", plain)
        # What crosses to the provider is not what is shown.
        self.assertIn("placeholder-header", config.model_dump_json())


class ProviderTests(unittest.TestCase):
    def test_the_provider_is_roomkit_s_and_says_what_roomkit_s_own_says(self) -> None:
        provider = OpenAI(OpenAIConfig(api_key=KEY, model="gpt-4o"))
        self.assertIsInstance(provider, AIProvider)
        said = {
            "name": provider.name,
            "model_name": provider.model_name,
            "supports_vision": provider.supports_vision,
            "supports_streaming": provider.supports_streaming,
            "supports_structured_streaming": provider.supports_structured_streaming,
            "supports_response_schema": provider.supports_response_schema,
            "supports_response_schema_with_tools": provider.supports_response_schema_with_tools,
            "supports_deferred_tools": provider.supports_deferred_tools,
            "context_window": provider.context_window,
        }
        self.assertEqual(
            said,
            {
                "name": "OpenAIAIProvider",
                "model_name": "gpt-4o",
                "supports_vision": True,
                "supports_streaming": True,
                "supports_structured_streaming": True,
                "supports_response_schema": True,
                "supports_response_schema_with_tools": True,
                "supports_deferred_tools": False,
                "context_window": 128000,
            },
        )
        # The name is the one RoomKit's own provider goes by: what RoomKit
        # records under it does not change with the provider.
        self.assertEqual(provider.name, RoomKitProvider.__name__)

    def test_the_catalogue_is_roomkit_s_own_in_roomkit_s_models(self) -> None:
        models = OpenAI.available_models()
        self.assertTrue(all(isinstance(model, ModelInfo) for model in models))
        self.assertEqual(models, RoomKitProvider.available_models())
        provider = OpenAI(OpenAIConfig(api_key=KEY, model="gpt-4o"))
        entry = provider.catalog_entry()
        self.assertEqual(entry, next(model for model in models if model.id == "gpt-4o"))
        # What RoomKit prices a turn with.
        cost = entry.pricing.cost_for({"input_tokens": 1000, "output_tokens": 10})
        self.assertAlmostEqual(cost, 0.0026)
        local = OpenAIConfig(api_key=KEY, model="local-model", base_url="http://localhost:1/v1")
        self.assertIsNone(OpenAI(local).catalog_entry())
        self.assertIsNone(OpenAI(local).context_window)

    def test_roomkit_s_own_configuration_builds_the_same_provider(self) -> None:
        for members in TAKEN:
            with self.subTest(members=members):
                own = OpenAI(OpenAIConfig(**members))
                moved = OpenAI(RoomKitConfig(**members))
                self.assertEqual(
                    (moved.model_name, moved.supports_vision, moved.context_window),
                    (own.model_name, own.supports_vision, own.context_window),
                )

    def test_another_configuration_is_refused(self) -> None:
        class DerivedConfig(OpenAIConfig):
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
                OpenAI(config)

    def test_a_value_the_provider_s_reader_refuses_is_a_value_error(self) -> None:
        # RoomKit's configuration takes a time below zero; no clock counts it.
        config = OpenAIConfig(api_key=KEY, model="m", timeout=-1)
        with self.assertRaises(ValueError) as refused:
            OpenAI(config)
        self.assertNotIn(KEY, str(refused.exception))

    def test_a_provider_that_cannot_be_built_fails_as_roomkit_s_provider_error(self) -> None:
        config = OpenAIConfig(api_key=KEY, model="m", base_url="https://llm.example/v1?route=1")
        with self.assertRaises(ProviderError) as refused:
            OpenAI(config)
        self.assertTrue(refused.exception.before_request)
        self.assertEqual(refused.exception.provider, "openai")
        self.assertNotIn(KEY, str(refused.exception))

    def test_the_version_is_the_project_s(self) -> None:
        project = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]
        self.assertEqual(roomkit_providers.__version__, project["version"])


class CallTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        self.telemetry = MockTelemetryProvider()

    def provider(self, **members: object) -> OpenAI:
        provider = OpenAI(OpenAIConfig(**{**local_config(self.fake.port), **members}))
        # As RoomKit's AI channel hands a provider its telemetry.
        provider._telemetry = self.telemetry
        return provider

    def first_tokens(self) -> list[dict]:
        return [m for m in self.telemetry.metrics if m["name"] == "roomkit.llm.ttfb_ms"]

    async def test_a_context_is_answered_with_roomkit_s_response(self) -> None:
        self.fake.answer(Whole(200, JSON, completion("Hello there.")))
        context = AIContext(
            messages=[AIMessage(role="user", content="Hello")],
            system_prompt="You answer in one sentence.",
            # The key a context carries for itself, and what a host left
            # beside it.
            metadata={"api_key": "placeholder-of-one-request", "session": object()},
        )
        response = await self.provider().generate(context)
        self.assertIsInstance(response, AIResponse)
        self.assertEqual(
            (response.content, response.finish_reason, response.usage),
            ("Hello there.", "stop", {"input_tokens": 12, "output_tokens": 5}),
        )
        self.assertEqual((response.tasks, response.observations, response.tool_calls), ([], [], []))
        (seen,) = self.fake.seen
        body = json.loads(seen.body)
        self.assertEqual(body["messages"][0]["role"], "system")
        # OpenAI's provider reads no key of one request, as RoomKit's own
        # does not: the configuration's is sent, and nothing of the other.
        self.assertEqual(seen.headers["authorization"], f"Bearer {KEY}")
        self.assertNotIn("placeholder-of-one-request", seen.body.decode())

    async def test_a_switch_of_the_configuration_reaches_the_wire(self) -> None:
        self.fake.answer(Whole(200, JSON, completion("Yes.")), Whole(200, JSON, completion("Yes.")))
        await self.provider().generate(HELLO)
        await self.provider(use_max_completion_tokens=True).generate(HELLO)
        left, stated = (set(json.loads(seen.body)) for seen in self.fake.seen)
        cap = {"max_tokens", "max_completion_tokens"}
        self.assertEqual((left & cap, stated & cap), ({"max_tokens"}, {"max_completion_tokens"}))

    async def test_a_stream_gives_roomkit_s_events(self) -> None:
        self.fake.answer(
            Reads(200, EVENT_STREAM, reasoning_then_text()),
            Reads(200, EVENT_STREAM, call_events("lookup_order", '{"number": "A-1"}')),
            Reads(200, EVENT_STREAM, events("Hel", "lo.")),
        )
        provider = self.provider()
        said = [event async for event in provider.generate_structured_stream(HELLO)]
        kinds = [StreamThinkingDelta, StreamTextDelta, StreamDone]
        self.assertEqual([type(event) for event in said], kinds)
        self.assertEqual((said[0].thinking, said[1].text), ("They greet me.", "Hello."))
        called = [event async for event in provider.generate_structured_stream(HELLO)]
        kinds = [StreamToolCallDelta, StreamToolCall, StreamDone]
        self.assertEqual([type(event) for event in called], kinds)
        self.assertEqual(called[1].arguments, {"number": "A-1"})
        self.assertFalse(called[1].partial)
        self.assertEqual([piece async for piece in provider.generate_stream(HELLO)], ["Hel", "lo."])

    async def test_the_models_an_endpoint_lists_are_roomkit_s(self) -> None:
        listed = {"object": "list", "data": [{"id": "gpt-4o"}, {"id": "local-model"}]}
        self.fake.answer(Whole(200, JSON, json.dumps(listed).encode()))
        models = await self.provider().list_models()
        self.assertTrue(all(isinstance(model, ModelInfo) for model in models))
        self.assertEqual([model.id for model in models], ["gpt-4o", "local-model"])
        self.assertEqual(models[0].display_name, "GPT-4o")

    async def test_a_failure_is_roomkit_s_provider_error_with_its_attributes(self) -> None:
        refusal = {"error": {"message": "Rate limit reached.", "type": "rate_limit_error"}}
        self.fake.answer(
            Whole(429, JSON, json.dumps(refusal).encode()),
            Whole(429, JSON, json.dumps(refusal).encode()),
        )
        provider = self.provider()
        with self.assertRaises(ProviderError) as whole:
            await provider.generate(HELLO)
        with self.assertRaises(ProviderError) as streamed:
            await anext(provider.generate_structured_stream(HELLO))
        said = "the provider answered HTTP 429 (rate_limit_error): Rate limit reached."
        for failed in (whole.exception, streamed.exception):
            self.assertIs(type(failed), ProviderError)
            self.assertEqual(failed.args, (said,))
            self.assertEqual(str(failed), f"openai (429): {said}")
            self.assertEqual(
                (failed.retryable, failed.provider, failed.status_code, failed.before_request),
                (True, "openai", 429, False),
            )
            self.assertIsNone(failed.context_overflow)

    async def test_a_response_schema_s_failure_is_roomkit_s_with_its_reason(self) -> None:
        schema = {
            "type": "object",
            "properties": {"label": {"type": "string"}},
            "required": ["label"],
            "additionalProperties": False,
        }
        context = HELLO.model_copy(update={"response_schema": schema})
        with self.assertRaises(ResponseSchemaError) as refused:
            await self.provider(supports_response_schema=False).generate(context)
        error = refused.exception
        self.assertEqual(
            (error.reason, error.before_request, error.retryable), ("unsupported", True, False)
        )
        self.assertEqual(self.fake.seen, [])

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
        self.assertEqual(self.fake.seen, [])

    async def test_the_first_token_of_a_whole_answer_is_recorded_once(self) -> None:
        self.fake.answer(Whole(200, JSON, completion("Yes.")), Whole(401, JSON, b"{}"))
        provider = self.provider()
        await provider.generate(HELLO)
        (metric,) = self.first_tokens()
        self.assertEqual(metric["unit"], "ms")
        self.assertEqual(
            metric["attributes"], {"provider": "OpenAIAIProvider", "model": "local-model"}
        )
        # A request that failed recorded none.
        with self.assertRaises(ProviderError):
            await provider.generate(HELLO)
        self.assertEqual(len(self.first_tokens()), 1)

    async def test_the_first_token_of_a_stream_is_its_first_text_or_reasoning(self) -> None:
        cases = [
            ("text", events("Hel", "lo.", "!"), 1),
            ("reasoning then text", reasoning_then_text(), 1),
            ("a call alone", call_events("lookup_order", '{"number": "A-1"}'), 0),
        ]
        for label, reads, recorded in cases:
            with self.subTest(stream=label):
                self.telemetry.metrics.clear()
                self.fake.answer(Reads(200, EVENT_STREAM, reads))
                seen: list[int] = []
                async for _ in self.provider().generate_structured_stream(HELLO):
                    seen.append(len(self.first_tokens()))
                self.assertEqual(len(self.first_tokens()), recorded)
                if recorded:
                    # It was recorded when the first event came, not at the end.
                    self.assertEqual(seen[0], 1)
        self.telemetry.metrics.clear()
        self.fake.answer(Reads(200, EVENT_STREAM, events("Hel", "lo.")))
        async for _ in self.provider().generate_stream(HELLO):
            pass
        self.assertEqual(len(self.first_tokens()), 1)

    async def test_a_provider_no_channel_handed_telemetry_records_nothing(self) -> None:
        self.fake.answer(Whole(200, JSON, completion("Yes.")))
        provider = OpenAI(OpenAIConfig(**local_config(self.fake.port)))
        self.assertEqual((await provider.generate(HELLO)).content, "Yes.")
        self.assertEqual(self.first_tokens(), [])

    async def test_cancelling_a_call_or_closing_a_stream_closes_its_connection(self) -> None:
        self.fake.answer(Silent(), Reads(200, EVENT_STREAM, events("Hel")[:1], then="silent"))
        provider = self.provider()
        asking = asyncio.create_task(provider.generate(HELLO))
        await self.fake.received()
        asking.cancel()
        await self.fake.closed()
        with self.assertRaises(asyncio.CancelledError):
            await asking
        stream = provider.generate_structured_stream(HELLO)
        self.assertEqual((await anext(stream)).text, "Hel")
        await stream.aclose()
        await self.fake.closed()
        self.assertIsNone(await provider.close())


if __name__ == "__main__":
    unittest.main()
