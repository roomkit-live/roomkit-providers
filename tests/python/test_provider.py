"""What every provider shares, tested with no vendor and no HTTP: RoomKit's
provider interface over a scripted provider of the contract.

The scripted provider is the contract's own mock, a port of RoomKit's
``MockAIProvider``: what it is asked and what it answers are compared with
what RoomKit's writes and gives for the same script. It is built into the
native module for these tests only.
"""

from __future__ import annotations

import asyncio
import inspect
import json
import os
import threading
import unittest
from typing import Any

from pydantic import SecretStr
from roomkit.providers.ai import (
    AIContext,
    AIImagePart,
    AIMessage,
    AIProvider,
    AIResponse,
    AITextPart,
    AIThinkingPart,
    AITool,
    AIToolCall,
    AIToolCallPart,
    AIToolResultPart,
    MockAIProvider,
    ProviderError,
    ResponseSchemaError,
    ServedCall,
)
from roomkit.telemetry import MockTelemetryProvider
from roomkit_providers import _native
from roomkit_providers.llm._provider import Provider, request_key

if not hasattr(_native, "_scripted"):
    raise unittest.SkipTest(
        "the native module was built without its scripted provider (the binding's "
        "`testing` feature): the tests of what every provider shares need it"
    )

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
ONE_REQUEST_KEY = "placeholder-of-one-request"
SCHEMA = {
    "type": "object",
    "properties": {"label": {"type": "string"}},
    "required": ["label"],
    "additionalProperties": False,
}
# What crosses of a context: every member a provider reads but its metadata.
REQUEST_MEMBERS = {
    "messages",
    "system_prompt",
    "temperature",
    "max_tokens",
    "thinking_budget",
    "enable_thinking",
    "reasoning_effort",
    "tools",
    "response_schema",
}
# A response with everything the contract carries: reasoning in signed and
# redacted blocks, a call the provider served itself, another the host runs.
WHOLE = AIResponse(
    content="Your order left this morning.",
    thinking="They ask of an order. I looked it up.",
    thinking_signature="signature-2",
    thinking_parts=[
        AIThinkingPart(thinking="They ask of an order.", signature="signature-1"),
        AIThinkingPart(thinking="", redacted="opaque-block"),
        AIThinkingPart(thinking=" I looked it up.", signature="signature-2"),
    ],
    finish_reason="tool_calls",
    usage={"input_tokens": 21, "output_tokens": 9, "cache_read_input_tokens": 4},
    metadata={"model": "served-model"},
    tool_calls=[
        AIToolCall(
            id="call_search",
            name="web_search",
            arguments={"query": "order A-1"},
            served=ServedCall(result="found", is_error=False),
        ),
        AIToolCall(
            id="call_lookup",
            name="lookup_order",
            arguments={"number": "A-1"},
            metadata={"thought_signature": "signature-3"},
        ),
    ],
)


def written(response: AIResponse) -> dict[str, Any]:
    """A response in the contract's JSON form: RoomKit's, without the tasks
    and observations RoomKit adds."""
    return response.model_dump(mode="json", exclude={"tasks", "observations"})


def scripted(*responses: AIResponse, **script: Any) -> tuple[Provider, Any]:
    """A provider over the scripted provider `script` writes, and that
    scripted provider, which says what it was asked."""
    script["responses"] = [written(response) for response in responses]
    held = _native._scripted(json.dumps(script))
    return Provider(held.provider()), held


def a_whole_turn() -> AIContext:
    """A context with everything the contract carries of a request."""
    lookup = AITool(
        name="lookup_order",
        description="Finds an order by its number.",
        parameters={"type": "object", "properties": {"number": {"type": "string"}}},
        tags=["orders"],
        changes_state=True,
    )
    held_back = AITool(name="refund_order", description="Refunds an order.", defer_loading=True)
    asked = AIMessage(
        role="user",
        content=[
            AITextPart(text="Where is this order?"),
            AIImagePart(url="data:image/png;base64,iVBORw0KGgo=", mime_type="image/png"),
        ],
        metadata={"author": "customer"},
    )
    reasoned = AIMessage(
        role="assistant",
        content=[
            AIThinkingPart(thinking="They ask of an order.", signature="signature-1"),
            AIThinkingPart(thinking="", redacted="opaque-block"),
            AITextPart(text="Let me look."),
            AIToolCallPart(
                id="call_find",
                name="find_tools",
                arguments={"need": "orders"},
                metadata={"thought_signature": "signature-2"},
            ),
        ],
    )
    found = AIMessage(
        role="tool",
        content=[
            AIToolResultPart(
                tool_call_id="call_find",
                name="find_tools",
                result="refund_order: refunds an order",
                outcome="served",
                references=["refund_order"],
                structured_content={"tools": ["refund_order"]},
            )
        ],
    )
    return AIContext(
        messages=[asked, reasoned, found],
        system_prompt="You answer in one sentence.",
        temperature=0.25,
        max_tokens=512,
        thinking_budget=2048,
        enable_thinking=True,
        reasoning_effort="high",
        tools=[lookup, held_back],
        response_schema=SCHEMA,
        metadata={"api_key": ONE_REQUEST_KEY, "session": object()},
    )


class AnswersTests(unittest.TestCase):
    def test_every_answer_comes_from_the_native_provider(self) -> None:
        def said(provider: Provider) -> tuple:
            return (
                provider.supports_vision,
                provider.supports_streaming,
                provider.supports_structured_streaming,
                provider.supports_response_schema,
                provider.supports_response_schema_with_tools,
                provider.supports_deferred_tools,
            )

        plain, _ = scripted()
        self.assertIsInstance(plain, AIProvider)
        self.assertEqual(said(plain), (False,) * 6)
        able, _ = scripted(
            vision=True,
            streaming=True,
            response_schema=True,
            response_schema_with_tools=True,
            deferred_tools=True,
        )
        self.assertEqual(said(able), (True,) * 6)
        # The provider's own name, not the class's that holds it.
        roomkit_s = MockAIProvider()
        self.assertEqual(
            (plain.name, plain.model_name, plain.context_window),
            (roomkit_s.name, roomkit_s.model_name, roomkit_s.context_window),
        )
        self.assertEqual(plain.catalog_entry(), roomkit_s.catalog_entry())
        # A provider's class names its catalogue: this one names none.
        self.assertEqual(Provider.available_models(), [])

    def test_each_member_has_the_signature_of_roomkit_s_provider(self) -> None:
        members = [name for name in vars(AIProvider) if not name.startswith("_")]
        self.assertGreaterEqual(len(members), 15, members)
        for name in members:
            with self.subTest(member=name):
                own, roomkit_s = getattr(Provider, name), getattr(AIProvider, name)
                self.assertIs(type(own), type(roomkit_s))
                if isinstance(own, property):
                    own, roomkit_s = own.fget, roomkit_s.fget
                self.assertEqual(inspect.signature(own), inspect.signature(roomkit_s))
                self.assertEqual(
                    (inspect.iscoroutinefunction(own), inspect.isasyncgenfunction(own)),
                    (
                        inspect.iscoroutinefunction(roomkit_s),
                        inspect.isasyncgenfunction(roomkit_s),
                    ),
                )

    def test_the_key_of_one_request_is_read_as_roomkit_reads_it(self) -> None:
        def context(**metadata: Any) -> AIContext:
            return AIContext(messages=HELLO.messages, metadata=metadata)

        self.assertEqual(request_key(context(api_key=ONE_REQUEST_KEY)), ONE_REQUEST_KEY)
        self.assertEqual(request_key(context(api_key=SecretStr(ONE_REQUEST_KEY))), ONE_REQUEST_KEY)
        for none in (context(), context(api_key=""), context(api_key=7), context(key="other")):
            self.assertIsNone(request_key(none))


class CrossingTests(unittest.IsolatedAsyncioTestCase):
    async def test_a_request_reaches_the_provider_with_all_the_contract_carries(self) -> None:
        provider, held = scripted(
            AIResponse(content='{"label": "order"}'),
            response_schema=True,
            response_schema_with_tools=True,
        )
        context = a_whole_turn()
        await provider.generate(context)
        [event async for event in provider.generate_structured_stream(context)]
        roomkit_s = context.model_dump(mode="json", include=REQUEST_MEMBERS)
        whole, streamed = json.loads(held.asked_json())
        # Deferred tools, references, signed and redacted reasoning: each
        # member as RoomKit writes it. Of the metadata, the key alone, which
        # the contract writes masked.
        for asked in (whole, streamed):
            self.assertEqual(asked.pop("metadata"), {"api_key": "**********"})
            self.assertEqual(asked, roomkit_s)
        self.assertTrue(whole["tools"][1]["defer_loading"])
        self.assertEqual(whole["messages"][2]["content"][0]["references"], ["refund_order"])

    async def test_the_key_of_one_request_crosses_apart_and_reaches_the_provider(self) -> None:
        provider, held = scripted()
        keyed = AIContext(messages=HELLO.messages, metadata={"api_key": ONE_REQUEST_KEY})
        secret = AIContext(
            messages=HELLO.messages, metadata={"api_key": SecretStr(ONE_REQUEST_KEY)}
        )
        await provider.generate(keyed)
        await provider.generate(HELLO)
        [event async for event in provider.generate_structured_stream(secret)]
        [piece async for piece in provider.generate_stream(keyed)]
        self.assertEqual(held.keys(), [ONE_REQUEST_KEY, None, ONE_REQUEST_KEY, ONE_REQUEST_KEY])
        # It crossed apart: no request's JSON form holds it.
        self.assertNotIn(ONE_REQUEST_KEY, held.asked_json())

    async def test_an_answer_comes_back_with_all_the_contract_carries(self) -> None:
        provider, _ = scripted(WHOLE)
        answered = await provider.generate(HELLO)
        self.assertIsInstance(answered, AIResponse)
        self.assertEqual(answered, WHOLE)
        self.assertEqual(answered.tool_calls[0].served, ServedCall(result="found"))
        self.assertEqual(answered.thinking_parts[1].redacted, "opaque-block")

    async def test_a_stream_gives_the_events_roomkit_s_own_mock_gives(self) -> None:
        options = {"streaming": True, "tool_call_delta_chunks": 3}
        plain = AIResponse(content="Hello there.", usage={"input_tokens": 3, "output_tokens": 2})
        kinds = set()
        for label, response in (("everything", WHOLE), ("text", plain)):
            with self.subTest(response=label):
                provider, _ = scripted(response, **options)
                roomkit_s = MockAIProvider(ai_responses=[response], **options)
                given = [event async for event in provider.generate_structured_stream(HELLO)]
                expected = [event async for event in roomkit_s.generate_structured_stream(HELLO)]
                self.assertEqual(given, expected)
                self.assertEqual([type(event) for event in given], [type(e) for e in expected])
                kinds |= {type(event).__name__ for event in given}
                text = [piece async for piece in provider.generate_stream(HELLO)]
                self.assertEqual("".join(text), response.content)
        self.assertEqual(
            kinds,
            {
                "StreamThinkingDelta",
                "StreamTextDelta",
                "StreamToolCallDelta",
                "StreamToolCall",
                "StreamDone",
            },
        )

    async def test_the_models_a_provider_lists_are_roomkit_s(self) -> None:
        provider, _ = scripted()
        self.assertEqual(await provider.list_models(), MockAIProvider.available_models())


class FailureTests(unittest.IsolatedAsyncioTestCase):
    async def failures(self, provider: Provider, context: AIContext) -> list[ProviderError]:
        """What a whole answer and a stream each fail with."""
        with self.assertRaises(ProviderError) as whole:
            await provider.generate(context)
        with self.assertRaises(ProviderError) as streamed:
            await anext(provider.generate_structured_stream(context))
        return [whole.exception, streamed.exception]

    async def test_a_provider_s_failure_is_roomkit_s_with_each_attribute(self) -> None:
        refusals = [
            {
                "message": "The server is overloaded.",
                "retryable": True,
                "provider": "scripted",
                "status_code": 529,
                "context_overflow": None,
                "before_request": False,
            },
            {
                "message": "The context is too long.",
                "retryable": False,
                "provider": "another",
                "status_code": 400,
                "context_overflow": True,
                "before_request": False,
            },
            {
                "message": "A tool's name is refused.",
                "retryable": False,
                "provider": "scripted",
                "status_code": None,
                "context_overflow": False,
                "before_request": True,
            },
        ]
        for refusal in refusals:
            with self.subTest(refusal=refusal["message"]):
                provider, _ = scripted(failure=refusal)
                raised = await self.failures(provider, HELLO)
                with self.assertRaises(ProviderError) as listing:
                    await provider.list_models()
                for error in (*raised, listing.exception):
                    self.assertIs(type(error), ProviderError)
                    said = {
                        "message": error.args[0],
                        "retryable": error.retryable,
                        "provider": error.provider,
                        "status_code": error.status_code,
                        "context_overflow": error.context_overflow,
                        "before_request": error.before_request,
                    }
                    self.assertEqual(said, refusal)
                    # Nothing of how it crossed shows.
                    self.assertIsNone(error.__cause__)

    async def test_a_response_schema_s_failure_is_roomkit_s_with_its_reason(self) -> None:
        asking = HELLO.model_copy(update={"response_schema": SCHEMA})
        cases = [
            ("unsupported", {}, AIResponse(content='{"label": "a"}')),
            ("invalid_json", {"response_schema": True}, AIResponse(content="A label.")),
            (
                "truncated",
                {"response_schema": True},
                AIResponse(content='{"label": "a', finish_reason="length"),
            ),
            (
                "refusal",
                {"response_schema": True},
                AIResponse(content="", finish_reason="refusal"),
            ),
        ]
        for reason, options, response in cases:
            with self.subTest(reason=reason):
                provider, _ = scripted(response, **options)
                roomkit_s = MockAIProvider(ai_responses=[response], **options)
                with self.assertRaises(ResponseSchemaError) as expected:
                    await roomkit_s.generate(asking)
                self.assertEqual(expected.exception.reason, reason)
                # Its words are RoomKit's, but those of the JSON parser,
                # which each language's quotes in brackets.
                said = expected.exception.args[0].partition(" (")[0]
                for error in await self.failures(provider, asking):
                    self.assertIs(type(error), ResponseSchemaError)
                    self.assertEqual(
                        (error.reason, error.retryable, error.before_request),
                        (reason, expected.exception.retryable, expected.exception.before_request),
                    )
                    self.assertEqual(error.args[0].partition(" (")[0], said)

    async def test_a_request_the_contract_refuses_fails_before_the_request(self) -> None:
        # RoomKit takes any role; the contract knows four. The refusal is the
        # failure an AI channel catches, and the provider is asked nothing.
        provider, held = scripted()
        context = AIContext(messages=[AIMessage(role="developer", content="Be brief.")])
        for error in await self.failures(provider, context):
            self.assertIs(type(error), ProviderError)
            self.assertEqual(
                (error.before_request, error.retryable, error.status_code), (True, False, None)
            )
            self.assertIn("developer", error.args[0])
        self.assertEqual(held.keys(), [])


class RefusalTests(unittest.IsolatedAsyncioTestCase):
    async def refusals(self, context: AIContext) -> list[ProviderError]:
        provider, held = scripted()
        with self.assertRaises(ProviderError) as whole:
            await provider.generate(context)
        with self.assertRaises(ProviderError) as streamed:
            await anext(provider.generate_structured_stream(context))
        self.assertEqual(held.keys(), [])
        return [whole.exception, streamed.exception]

    async def test_a_refusal_s_words_are_bounded(self) -> None:
        # The reader quotes what it refuses, however long.
        role = "developer " * 500_000
        context = AIContext(messages=[AIMessage(role=role, content="Be brief.")])
        for error in await self.refusals(context):
            self.assertLessEqual(len(error.args[0]), 500)
            self.assertTrue(error.args[0].startswith("the request is refused: "))
            self.assertTrue(error.before_request)

    async def test_a_context_with_no_json_form_is_refused_before_the_request(self) -> None:
        handle = object()
        in_a_message = AIMessage(role="user", content="Hello", metadata={"handle": handle})
        in_a_call = AIMessage(
            role="assistant",
            content=[AIToolCallPart(id="call_1", name="lookup", arguments={"handle": handle})],
        )
        for message in (in_a_message, in_a_call):
            for error in await self.refusals(AIContext(messages=[message])):
                self.assertIs(type(error), ProviderError)
                self.assertEqual(
                    (error.before_request, error.retryable, error.status_code), (True, False, None)
                )
                self.assertTrue(error.args[0].startswith("the request is refused: "))


class DefectTests(unittest.IsolatedAsyncioTestCase):
    """A provider that panics fails as a provider: the failure an AI channel
    catches, never an exception outside ``Exception``, never an answer."""

    async def test_a_panic_in_a_call_is_a_provider_error(self) -> None:
        provider, _ = scripted(panics=True)
        calls = (
            provider.generate(HELLO),
            provider.list_models(),
            anext(provider.generate_structured_stream(HELLO)),
        )
        for call in calls:
            with self.assertRaises(ProviderError) as failed:
                await call
            self.assertIs(type(failed.exception), ProviderError)
            self.assertFalse(failed.exception.retryable)
        # The thread that ran the call goes on.
        sound, _ = scripted()
        self.assertEqual((await sound.generate(HELLO)).content, "Hello from AI")

    async def test_a_panic_while_a_stream_is_read_fails_the_stream(self) -> None:
        options = {"streaming": True, "tool_call_delta_chunks": 3}
        sound, _ = scripted(WHOLE, **options)
        whole = [event async for event in sound.generate_structured_stream(HELLO)]
        self.assertGreaterEqual(len(whole), 10)
        provider, _ = scripted(WHOLE, panics_after=3, **options)
        given = []
        with self.assertRaises(ProviderError) as failed:
            async for event in provider.generate_structured_stream(HELLO):
                given.append(event)
        self.assertEqual(given, whole[:3])
        self.assertFalse(failed.exception.retryable)
        # The text of a stream that was cut is no answer.
        with self.assertRaises(ProviderError):
            [piece async for piece in provider.generate_stream(HELLO)]


class LifetimeTests(unittest.IsolatedAsyncioTestCase):
    async def test_cancelling_a_call_drops_it_in_the_provider(self) -> None:
        provider, held = scripted(silent=True)
        asking = asyncio.create_task(provider.generate(HELLO))
        await held.begun(1)
        asking.cancel()
        await held.dropped(1)
        with self.assertRaises(asyncio.CancelledError):
            await asking
        listing = asyncio.create_task(provider.list_models())
        await held.begun(2)
        listing.cancel()
        await held.dropped(2)

    async def test_cancelling_a_stream_s_read_drops_its_generation_in_the_provider(self) -> None:
        provider, held = scripted(silent=True)
        stream = provider.generate_structured_stream(HELLO)
        reading = asyncio.create_task(anext(stream))
        await held.begun(1)
        reading.cancel()
        await held.dropped(1)
        with self.assertRaises(asyncio.CancelledError):
            await reading

    async def test_closing_a_stream_while_it_is_read_ends_the_read(self) -> None:
        # The read and the reading both see the stream closed, in either
        # order: the read ends with nothing, never as a stream that failed.
        provider, held = scripted(silent=True)
        request = HELLO.model_dump_json(include={"messages"})
        for turn in range(1, 301):
            stream = provider._native.stream(request, None)
            reading = asyncio.ensure_future(stream.next())
            await held.begun(turn)
            stream.close()
            self.assertIsNone(await reading, turn)
            self.assertIsNone(await stream.next(), turn)

    @unittest.skipUnless(os.path.isdir("/proc/self/task"), "the threads of a process, in /proc")
    async def test_the_calls_run_on_a_few_threads_whatever_their_number(self) -> None:
        workers, lookups = _native._threads()
        self.assertLessEqual(workers + lookups, 8)
        provider, _ = scripted(WHOLE, streaming=True, tool_call_delta_chunks=3)

        async def reads() -> int:
            return len([event async for event in provider.generate_structured_stream(HELLO)])

        def own_threads() -> int:
            return len(os.listdir("/proc/self/task")) - threading.active_count()

        await reads()
        counts = []
        for _ in range(5):
            await asyncio.gather(*(reads() for _ in range(200)))
            counts.append(own_threads())
        # The workers, and the one thread that hands what they read to
        # Python: no call brings a thread of its own.
        self.assertEqual(counts, [workers + 1] * 5)

    async def test_the_first_token_is_recorded_under_the_provider_s_own_name(self) -> None:
        telemetry = MockTelemetryProvider()
        provider, _ = scripted(WHOLE, streaming=True)
        provider._telemetry = telemetry
        await provider.generate(HELLO)
        [event async for event in provider.generate_structured_stream(HELLO)]
        metrics = [m for m in telemetry.metrics if m["name"] == "roomkit.llm.ttfb_ms"]
        self.assertEqual(len(metrics), 2)
        for metric in metrics:
            self.assertEqual(metric["unit"], "ms")
            self.assertEqual(
                metric["attributes"], {"provider": provider.name, "model": provider.model_name}
            )


if __name__ == "__main__":
    unittest.main()
