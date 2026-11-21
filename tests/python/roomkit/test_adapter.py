"""The adapter as a RoomKit program holds it: RoomKit's provider interface,
RoomKit's models and RoomKit's exceptions, over the Rust provider."""

from __future__ import annotations

import asyncio
import json
import unittest

import adapter_tests  # noqa: F401
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
from roomkit.providers.openai import OpenAIConfig
from roomkit.telemetry import MockTelemetryProvider
from roomkit_providers.openai import OpenAIConfig as PackageConfig
from roomkit_providers.roomkit import OpenAIAIProvider

KEY = "placeholder-not-a-key"
HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])


def reasoning_then_text() -> list[bytes]:
    """The reads of a streamed answer that reasons in a field, then says
    "Hello.", then ends."""
    first, *rest = events("Hello.")
    thought = {"index": 0, "delta": {"reasoning_content": "They greet me."}}
    chunk = {"id": "chatcmpl-1", "model": "local-model", "choices": [thought]}
    return [f"data: {json.dumps(chunk)}\n\n".encode(), first, *rest]


class SurfaceTests(unittest.TestCase):
    def test_the_adapter_is_roomkit_s_provider_built_from_roomkit_s_configuration(self) -> None:
        provider = OpenAIAIProvider(OpenAIConfig(api_key=KEY, model="gpt-4o"))
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
        # The name is the one RoomKit's own provider goes by.
        self.assertEqual(provider.name, RoomKitProvider.__name__)

    def test_the_catalogue_is_roomkit_s_own_in_roomkit_s_models(self) -> None:
        models = OpenAIAIProvider.available_models()
        self.assertTrue(all(isinstance(model, ModelInfo) for model in models))
        self.assertEqual(models, RoomKitProvider.available_models())
        provider = OpenAIAIProvider(OpenAIConfig(api_key=KEY, model="gpt-4o"))
        entry = provider.catalog_entry()
        self.assertEqual(entry, next(model for model in models if model.id == "gpt-4o"))
        # What RoomKit prices a turn with.
        cost = entry.pricing.cost_for({"input_tokens": 1000, "output_tokens": 10})
        self.assertAlmostEqual(cost, 0.0026)
        local = OpenAIConfig(api_key=KEY, model="local-model", base_url="http://localhost:1/v1")
        self.assertIsNone(OpenAIAIProvider(local).catalog_entry())
        self.assertIsNone(OpenAIAIProvider(local).context_window)

    def test_the_package_s_own_configuration_builds_the_same_provider(self) -> None:
        provider = OpenAIAIProvider(PackageConfig(api_key=KEY, model="gpt-4o"))
        self.assertEqual(provider.model_name, "gpt-4o")

    def test_another_configuration_is_refused(self) -> None:
        class DerivedConfig(OpenAIConfig):
            region: str = "eu"

        for config in (DerivedConfig(api_key=KEY, model="m"), {"api_key": KEY, "model": "m"}):
            with self.assertRaises(TypeError):
                OpenAIAIProvider(config)

    def test_a_provider_that_cannot_be_built_fails_as_roomkit_s_provider_error(self) -> None:
        config = OpenAIConfig(api_key=KEY, model="m", base_url="https://llm.example/v1?route=1")
        with self.assertRaises(ProviderError) as refused:
            OpenAIAIProvider(config)
        self.assertTrue(refused.exception.before_request)
        self.assertEqual(refused.exception.provider, "openai")
        self.assertIsInstance(refused.exception.__cause__, roomkit_providers.ProviderError)


class CallTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        self.telemetry = MockTelemetryProvider()

    def provider(self, **members: object) -> OpenAIAIProvider:
        provider = OpenAIAIProvider(OpenAIConfig(**{**local_config(self.fake.port), **members}))
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
            metadata={"api_key": "placeholder-of-one-request"},
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
        # The key a context carries for itself is not the one sent, as with
        # RoomKit's own OpenAI provider, and nothing of it is sent.
        self.assertEqual(seen.headers["authorization"], f"Bearer {KEY}")
        self.assertNotIn("placeholder-of-one-request", seen.body.decode())

    async def test_a_switch_of_roomkit_s_configuration_reaches_the_wire(self) -> None:
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
            self.assertEqual(failed.args, (said,))
            self.assertEqual(str(failed), f"openai (429): {said}")
            self.assertEqual(
                (failed.retryable, failed.provider, failed.status_code, failed.before_request),
                (True, "openai", 429, False),
            )
            self.assertIsNone(failed.context_overflow)
            self.assertNotIsInstance(failed, roomkit_providers.ProviderError)
            self.assertIsInstance(failed.__cause__, roomkit_providers.ProviderError)

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

    async def test_a_context_the_contract_refuses_is_a_value_error(self) -> None:
        refused = {
            # RoomKit takes any role and its own provider sends it; the
            # contract knows four.
            "a role": AIContext(messages=[AIMessage(role="developer", content="Be brief.")]),
            # A context crosses whole, its metadata included, which RoomKit's
            # own provider does not read: a value that is no JSON stops it.
            "metadata that is no JSON": AIContext(
                messages=HELLO.messages, metadata={"session": object()}
            ),
        }
        provider = self.provider()
        for label, context in refused.items():
            with self.subTest(context=label):
                with self.assertRaises(ValueError):
                    await provider.generate(context)
                with self.assertRaises(ValueError):
                    await anext(provider.generate_structured_stream(context))
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
        provider = OpenAIAIProvider(OpenAIConfig(**local_config(self.fake.port)))
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
