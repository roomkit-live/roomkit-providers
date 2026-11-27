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
import subprocess
import sys
import textwrap
import threading
import traceback
import unittest
import weakref
from types import SimpleNamespace
from typing import Any
from unittest import mock
from urllib import parse

from pydantic import BaseModel, SecretStr, ValidationError
from roomkit import (
    AIChannel,
    ChannelCategory,
    InboundMessage,
    RoomKit,
    TextContent,
    WebSocketChannel,
)
from roomkit.models import RetryPolicy
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
from roomkit.providers.ai import base as roomkit_base
from roomkit.telemetry import MockTelemetryProvider
from roomkit_providers import _native
from roomkit_providers.llm import _config, _provider
from roomkit_providers.llm._config import Config
from roomkit_providers.llm._provider import Provider, request_key

if not hasattr(_native, "_scripted"):
    raise unittest.SkipTest(
        "the native module was built without its scripted provider (the binding's "
        "`testing` feature): the tests of what every provider shares need it"
    )

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
KEY = "placeholder-not-a-key"
ONE_REQUEST_KEY = "placeholder-of-one-request"
HEADER = "placeholder-header-value"
PASSWORD = "placeholder-password"
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
    return Provider._over(held.provider()), held


class StandInConfig(BaseModel):
    """What stands for RoomKit's configuration of the scripted provider: the
    same members, and a ``repr`` that hides nothing but the key."""

    api_key: SecretStr
    model: str = "mock"
    base_url: str | None = None
    headers: dict[str, str] | None = None
    refuses: str | None = None
    panics_when_built: bool = False
    expects_key: str | None = None
    name: str | None = None
    label: str | None = None
    decided: bool = False


class ScriptedConfig(Config, vendor="scripted", stands_for=StandInConfig, decided=("decided",)):
    """The configuration of the scripted provider, as a provider's module
    writes one: its members, and what its class statement says."""

    api_key: SecretStr
    model: str = "mock"
    base_url: str | None = None
    headers: dict[str, str] | None = None
    refuses: str | None = None
    panics_when_built: bool = False
    expects_key: str | None = None
    name: str | None = None
    label: str | None = None
    decided: bool = False


class Scripted(Provider[ScriptedConfig | StandInConfig], config=ScriptedConfig):
    """The scripted provider, as a provider's module writes one: a class
    statement, and nothing else."""


class OptionalKeyConfig(Config, vendor="scripted"):
    """The configuration of a provider whose key is optional: its own
    ``api_key`` member says so."""

    api_key: SecretStr | None = None
    model: str = "mock"
    base_url: str | None = None
    headers: dict[str, str] | None = None
    refuses: str | None = None
    expects_key: str | None = None
    expects_no_key: bool = False
    failure: dict[str, Any] | None = None
    streaming: bool = True


class OptionalKey(Provider[OptionalKeyConfig], config=OptionalKeyConfig):
    """A provider whose key is optional."""


class NoKeyConfig(Config, vendor="scripted"):
    """The configuration of a provider that takes no key: it has no such
    member."""

    model: str = "mock"
    base_url: str | None = None
    headers: dict[str, str] | None = None
    refuses: str | None = None
    expects_no_key: bool = True


class NoKey(Provider[NoKeyConfig], config=NoKeyConfig):
    """A provider that takes no key."""


def shown(error: BaseException) -> str:
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


def run_child(body: str) -> subprocess.CompletedProcess[str]:
    """Runs `body` in an interpreter of its own, which a test of the bridge
    may leave unusable, after what every such scenario starts with."""
    preamble = """
        import asyncio
        import gc
        import json
        import os
        import sys

        from roomkit.providers.ai import AIContext, AIMessage, AIResponse, ProviderError
        from roomkit_providers import _native
        from roomkit_providers.llm._provider import Provider

        HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
        HELLO_JSON = HELLO.model_dump_json(include={"messages"})


        def scripted(**script):
            held = _native._scripted(json.dumps(script))
            return Provider._over(held.provider()), held
    """
    script = textwrap.dedent(preamble) + textwrap.dedent(body)
    return subprocess.run(
        [sys.executable, "-X", "faulthandler", "-I", "-c", script],
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )


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
        # Each answer is the provider's own answer to that question.
        alone = [
            {"vision": True},
            {"streaming": True, "structured_streaming": False},
            {"structured_streaming": True},
            {"response_schema": True},
            {"response_schema_with_tools": True},
            {"deferred_tools": True},
        ]
        for at, script in enumerate(alone):
            with self.subTest(supports=script):
                expected = tuple(index == at for index in range(6))
                self.assertEqual(said(scripted(**script)[0]), expected)
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
        # It is read in the context's own metadata, and nowhere else.
        said = AIMessage(role="user", content="Hello", metadata={"api_key": ONE_REQUEST_KEY})
        elsewhere = AIContext(messages=[said], system_prompt=f"api_key={ONE_REQUEST_KEY}")
        self.assertIsNone(request_key(elsewhere))


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


class FirstTokenTests(unittest.IsolatedAsyncioTestCase):
    """The time to the first token is recorded by one rule, whatever the
    provider: a stream records it at its first text or reasoning that is not
    empty, a whole answer when it came, if it holds text or reasoning."""

    CALL = AIToolCall(id="call_lookup", name="lookup_order", arguments={"number": "A-1"})
    ANSWERS = {
        "text": (AIResponse(content="Yes."), 1),
        "reasoning, then a call": (
            AIResponse(content="", thinking="They ask of an order.", tool_calls=[CALL]),
            1,
        ),
        "a redacted block, then text": (
            AIResponse(
                content="Yes.",
                thinking_parts=[AIThinkingPart(thinking="", redacted="opaque-block")],
            ),
            1,
        ),
        "a redacted block, then a call": (
            AIResponse(
                content="",
                thinking_parts=[AIThinkingPart(thinking="", redacted="opaque-block")],
                tool_calls=[CALL],
            ),
            0,
        ),
        "a signature alone, then a call": (
            AIResponse(
                content="",
                thinking_parts=[AIThinkingPart(thinking="", signature="signature-1")],
                tool_calls=[CALL],
            ),
            0,
        ),
        "a call alone": (AIResponse(content="", tool_calls=[CALL]), 0),
    }

    def recording(self, response: AIResponse) -> tuple[Provider, MockTelemetryProvider]:
        provider, _ = scripted(response, streaming=True, tool_call_delta_chunks=2)
        provider._telemetry = MockTelemetryProvider()
        return provider, provider._telemetry

    @staticmethod
    def first_tokens(telemetry: MockTelemetryProvider) -> list[dict[str, Any]]:
        return [m for m in telemetry.metrics if m["name"] == "roomkit.llm.ttfb_ms"]

    async def test_a_stream_records_its_first_text_or_reasoning_and_nothing_else(self) -> None:
        for label, (response, recorded) in self.ANSWERS.items():
            with self.subTest(stream=label):
                provider, telemetry = self.recording(response)
                at = []
                async for event in provider.generate_structured_stream(HELLO):
                    at.append((event, len(self.first_tokens(telemetry))))
                self.assertEqual(len(self.first_tokens(telemetry)), recorded)
                # It is recorded when the first text or reasoning that is not
                # empty comes, and at no event before it.
                for event, count in at:
                    output = bool(getattr(event, "text", "") or getattr(event, "thinking", ""))
                    if not output:
                        continue
                    self.assertEqual(count, 1, event)
                    break
                before = [count for event, count in at if not hasattr(event, "text")][:1]
                if recorded and before and at[0][0].type != "text_delta":
                    first_output = next(
                        index
                        for index, (event, _) in enumerate(at)
                        if getattr(event, "text", "") or getattr(event, "thinking", "")
                    )
                    self.assertEqual([count for _, count in at[:first_output]], [0] * first_output)

    async def test_a_whole_answer_records_when_it_holds_text_or_reasoning(self) -> None:
        for label, (response, recorded) in self.ANSWERS.items():
            with self.subTest(answer=label):
                provider, telemetry = self.recording(response)
                await provider.generate(HELLO)
                self.assertEqual(len(self.first_tokens(telemetry)), recorded)

    async def test_the_value_is_the_time_the_answer_took_in_milliseconds(self) -> None:
        # The call reads the time when it asks, RoomKit when it records.
        asked = SimpleNamespace(monotonic=lambda: 100.0)
        answered = SimpleNamespace(monotonic=lambda: 100.25)
        for call in ("generate", "generate_structured_stream"):
            with self.subTest(call=call):
                provider, telemetry = self.recording(AIResponse(content="Yes."))
                with (
                    mock.patch.object(_provider, "time", asked),
                    mock.patch.object(roomkit_base, "_time", answered),
                ):
                    if call == "generate":
                        await provider.generate(HELLO)
                    else:
                        [event async for event in provider.generate_structured_stream(HELLO)]
                (metric,) = self.first_tokens(telemetry)
                self.assertEqual(metric["value"], 250.0)
                self.assertEqual(metric["unit"], "ms")
                self.assertEqual(
                    metric["attributes"], {"provider": provider.name, "model": provider.model_name}
                )


class LentKeyTests(unittest.IsolatedAsyncioTestCase):
    """The key a host lends one request is in no failure of that request,
    whatever built the failure."""

    LENT = AIContext(
        messages=[AIMessage(role="user", content="Hello")], metadata={"api_key": ONE_REQUEST_KEY}
    )

    @staticmethod
    def refusing(message: str) -> Provider:
        failure = {
            "message": message,
            "retryable": False,
            "provider": "scripted",
            "status_code": 401,
            "context_overflow": None,
            "before_request": False,
        }
        return scripted(failure=failure, streaming=True)[0]

    async def failures(self, provider: Provider, context: AIContext) -> list[ProviderError]:
        failed = []
        for call in (
            provider.generate(context),
            anext(provider.generate_structured_stream(context)),
            anext(provider.generate_stream(context)),
        ):
            with self.assertRaises(ProviderError) as raised:
                await call
            failed.append(raised.exception)
        return failed

    async def test_a_failure_that_repeats_the_key_of_its_request_hides_it(self) -> None:
        provider = self.refusing(f"Incorrect key provided: {ONE_REQUEST_KEY}. Check it.")
        for failed in await self.failures(provider, self.LENT):
            for said in (failed.args[0], str(failed), repr(failed), repr(failed.args)):
                self.assertNotIn(ONE_REQUEST_KEY, said)
            self.assertEqual(failed.args[0], "Incorrect key provided: [***]. Check it.")
            self.assertEqual(failed.status_code, 401)

    async def test_a_key_a_server_repeats_as_a_url_writes_it_is_hidden(self) -> None:
        """Whatever the key holds: a quote and a backslash, which a URL
        never writes as they are, and a hyphen, which one writer escapes and
        another does not."""
        key = 'placeholder-"of"\\one request'
        lent = AIContext(messages=HELLO.messages, metadata={"api_key": key})
        for written in (
            parse.quote(key, safe=""),
            parse.quote(key, safe="").lower(),
            parse.quote(key, safe=" -").replace("-", "%2D", 1),
        ):
            with self.subTest(written=written):
                self.assertNotEqual(written, key)
                provider = self.refusing(f"Incorrect key provided: {written}. Check it.")
                for failed in await self.failures(provider, lent):
                    self.assertEqual(failed.args[0], "Incorrect key provided: [***]. Check it.")

    async def test_a_failure_keeps_the_words_of_a_request_that_lent_no_key(self) -> None:
        said = f"Incorrect key provided: {ONE_REQUEST_KEY}. Check it."
        for failed in await self.failures(self.refusing(said), HELLO):
            self.assertEqual(failed.args[0], said)

    async def test_a_refusal_built_before_the_request_hides_the_key_too(self) -> None:
        # What has no JSON form is named by its type: one named as the key.
        handle = type(ONE_REQUEST_KEY, (), {})()
        context = AIContext(
            messages=[AIMessage(role="user", content="Hello", metadata={"handle": handle})],
            metadata={"api_key": ONE_REQUEST_KEY},
        )
        provider, held = scripted()
        for failed in await self.failures(provider, context):
            self.assertTrue(failed.before_request)
            self.assertIn("[***]", failed.args[0])
            self.assertNotIn(ONE_REQUEST_KEY, str(failed))
        self.assertEqual(json.loads(held.asked_json()), [])


class ConstructionTests(unittest.TestCase):
    """What a provider's module gets without writing it: a provider built
    from its configuration alone, through the one class every provider
    shares, with what guards the way."""

    HELD: dict[str, Any] = {  # noqa: RUF012
        "api_key": KEY,
        "base_url": f"https://team:{PASSWORD}@llm.example/v1?token={PASSWORD}",
        "headers": {"X-Team-Token": HEADER},
    }

    def test_a_provider_is_built_from_its_configuration_and_names_itself(self) -> None:
        provider = Scripted(ScriptedConfig(api_key=KEY, expects_key=KEY, model="local-model"))
        self.assertIsInstance(provider, AIProvider)
        self.assertEqual((provider.name, provider.model_name), ("MockAIProvider", "local-model"))
        self.assertEqual(
            [model.id for model in Scripted.available_models()],
            [model.id for model in MockAIProvider.available_models()],
        )

    def test_the_key_crosses_apart_from_the_json_and_is_opened_in_native_code(self) -> None:
        crossed = []

        def recording(vendor: str, config_json: str, api_key: Any) -> Any:
            crossed.append((vendor, config_json, api_key))
            return build(vendor, config_json, api_key)

        build = _native.provider
        with mock.patch.object(_config._native, "provider", recording):
            Scripted(ScriptedConfig(api_key=KEY, expects_key=KEY))
        ((vendor, config_json, api_key),) = crossed
        self.assertEqual(vendor, "scripted")
        self.assertEqual(json.loads(config_json)["api_key"], "**********")
        # The key leaves Python as the object that masks it.
        self.assertIsInstance(api_key, SecretStr)
        self.assertNotIn(KEY, repr(api_key))
        # The provider is refused when another key crosses.
        with self.assertRaises(ProviderError):
            Scripted(ScriptedConfig(api_key=KEY, expects_key="another-key"))

    def test_a_provider_that_cannot_be_built_fails_as_roomkit_s_provider_error(self) -> None:
        for config in (ScriptedConfig, StandInConfig):
            with self.subTest(config=config.__name__):
                try:
                    Scripted(config(**self.HELD, refuses="the endpoint is not one"))
                except ProviderError as refused:
                    failed, frames = refused, shown(refused)
                else:
                    self.fail("the provider was built")
                self.assertIs(type(failed), ProviderError)
                self.assertEqual(failed.args[0], "the endpoint is not one")
                self.assertEqual((failed.provider, failed.before_request), ("scripted", True))
                # No frame the failure went through shows a secret of the
                # configuration.
                for secret in (KEY, PASSWORD, HEADER):
                    self.assertNotIn(secret, frames)

    def test_a_panic_while_a_provider_is_built_is_a_provider_error(self) -> None:
        with self.assertRaises(Exception) as failed:  # noqa: B017
            Scripted(ScriptedConfig(api_key=KEY, panics_when_built=True))
        self.assertIs(type(failed.exception), ProviderError)
        self.assertEqual(failed.exception.args[0], "the provider stopped on an internal error")
        self.assertEqual(failed.exception.provider, "scripted")
        self.assertFalse(failed.exception.retryable)

    def test_a_panic_while_a_configuration_is_read_is_a_provider_error(self) -> None:
        with self.assertRaises(Exception) as failed:  # noqa: B017
            ScriptedConfig(api_key=KEY, model="panics-as-it-is-read")
        self.assertIs(type(failed.exception), ProviderError)
        self.assertEqual(failed.exception.provider, "scripted")

    def test_a_provider_that_does_not_name_itself_as_its_vendor_states_is_refused(self) -> None:
        for misnamed in ({"name": "Scripted"}, {"label": ""}, {"label": "another"}):
            with self.subTest(**misnamed), self.assertRaises(ProviderError) as failed:
                Scripted(ScriptedConfig(api_key=KEY, **misnamed))
            self.assertIn("does not name itself", failed.exception.args[0])

    def test_a_configuration_shows_no_secret(self) -> None:
        said = repr(ScriptedConfig(**self.HELD))
        for secret in (KEY, PASSWORD, HEADER):
            self.assertNotIn(secret, said)
        self.assertIn("base_url='https://llm.example'", said)
        self.assertIn("headers={'X-Team-Token': '**********'}", said)

    def test_no_key_crosses_to_decide_a_switch(self) -> None:
        asked = []

        def recording(vendor: str, config_json: str) -> str:
            asked.append((vendor, json.loads(config_json)))
            return decide(vendor, config_json)

        decide = _native.config_form
        with mock.patch.object(_config._native, "config_form", recording):
            ScriptedConfig(api_key=KEY, model="decided-model", base_url="http://127.0.0.1:9")
        members = {"api_key": "", "model": "decided-model", "base_url": "http://127.0.0.1:9"}
        self.assertEqual(asked, [("scripted", members)])

    def test_a_switch_left_unset_is_decided_by_the_provider_s_own_rule(self) -> None:
        decided = [
            ScriptedConfig(api_key=KEY, model=model, **stated).decided
            for model, stated in (
                ("mock", {}),
                ("decided-model", {}),
                ("decided-model", {"decided": False}),
                ("mock", {"decided": True}),
            )
        ]
        self.assertEqual(decided, [False, True, False, True])

    def test_a_base_url_that_does_not_read_is_refused_without_being_shown(self) -> None:
        with self.assertRaises(ValidationError) as refused:
            ScriptedConfig(api_key=KEY, base_url=f"https://team:{PASSWORD}@[not-an-address]/v1")
        said = str(refused.exception)
        self.assertIn("base_url does not read as a URL", said)
        for secret in (KEY, PASSWORD):
            self.assertNotIn(secret, said)

    def test_a_refusal_never_shows_the_value_refused(self) -> None:
        with self.assertRaises(ValidationError) as refused:
            ScriptedConfig(api_key=KEY, headers={"X-Team-Token": [HEADER]})
        self.assertNotIn(HEADER, str(refused.exception))

    def test_roomkit_s_configuration_crosses_member_by_member(self) -> None:
        stated = {
            "api_key": KEY,
            "model": "local-model",
            "base_url": "http://127.0.0.1:9/v1",
            "headers": {"X-Team-Token": HEADER},
            "expects_key": KEY,
            "name": "MockAIProvider",
            "label": "scripted",
            "decided": True,
        }
        unstated = {"refuses", "panics_when_built"}
        self.assertEqual(set(stated) | unstated, set(StandInConfig.model_fields))
        crossed = []

        def recording(vendor: str, config_json: str, api_key: Any) -> Any:
            crossed.append((json.loads(config_json), api_key.get_secret_value()))
            return build(vendor, config_json, api_key)

        build = _native.provider
        with mock.patch.object(_config._native, "provider", recording):
            Scripted(StandInConfig(**stated))
            Scripted(ScriptedConfig(**stated))
        self.assertEqual(crossed[0], crossed[1])
        for name, value in stated.items():
            self.assertEqual(crossed[0][0][name], "**********" if name == "api_key" else value)

    def test_another_configuration_is_refused(self) -> None:
        class Derived(ScriptedConfig):
            region: str = "eu"

        class DerivedStandIn(StandInConfig):
            region: str = "eu"

        for config in (Derived(**self.HELD), DerivedStandIn(**self.HELD), object(), None):
            with self.subTest(config=type(config).__name__):
                try:
                    Scripted(config)
                except TypeError as refused:
                    frames = shown(refused)
                else:
                    self.fail("the configuration was taken")
                for secret in (KEY, PASSWORD, HEADER):
                    self.assertNotIn(secret, frames)

    def test_a_provider_s_class_names_its_configuration(self) -> None:
        with self.assertRaises(TypeError):

            class Unconfigured(Provider):
                pass

        with self.assertRaises(TypeError):

            class Unnamed(Config):
                model: str = "mock"

        with self.assertRaises(TypeError):

            class Deciding(Config, vendor="scripted", decided=("no_such_member",)):
                model: str = "mock"

        for keyword in ("rewritten", "urls"):
            with self.subTest(keyword=keyword), self.assertRaises(TypeError):

                class Naming(Config, vendor="scripted", **{keyword: ("no_such_member",)}):
                    model: str = "mock"

        # A class derived from a provider's is that provider's.
        class Derived(Scripted):
            pass

        self.assertEqual(Derived(ScriptedConfig(api_key=KEY)).name, "MockAIProvider")


class BridgeTests(unittest.IsolatedAsyncioTestCase):
    """The thread that hands the calls to Python, and what the threads that
    run them hold of Python."""

    async def test_a_stopped_call_lets_go_of_its_future_on_the_handing_thread(self) -> None:
        # No thread that runs a call lets go of a Python reference: the one
        # thread that hands the calls to Python does, under its lock, before
        # the call counts as over.
        provider, held = scripted(silent=True)
        request = HELLO.model_dump_json(include={"messages"})
        loop, released = asyncio.get_running_loop(), asyncio.Event()
        let_go: list[int] = []

        def seen(_: Any) -> None:
            let_go.append(threading.get_ident())
            loop.call_soon_threadsafe(released.set)

        asking = provider._native.generate(request, None)
        await held.begun(1)
        watching = weakref.ref(asking, seen)
        asking.cancel()
        del asking
        # Nothing else enters the native module meanwhile: a reference a
        # thread let go of off the lock would wait there for the next entry.
        await asyncio.wait_for(released.wait(), 10)
        self.assertIsNone(watching())
        self.assertEqual(len(let_go), 1)
        self.assertNotEqual(let_go[0], threading.get_ident())
        await held.dropped(1)

    def test_a_panic_while_a_call_is_handed_fails_that_call_and_no_other(self) -> None:
        result = run_child("""
            async def main():
                provider, _ = scripted(responses=[{"content": "Yes."}])
                assert (await provider.generate(HELLO)).content == "Yes."
                _native._handing_panics(1)
                try:
                    await provider.generate(HELLO)
                except ProviderError as failed:
                    print("failed:", failed.args[0], failed.retryable)
                # The thread that hands the calls goes on.
                print("then:", (await provider.generate(HELLO)).content)
                print("handing:", _native._handing(), _native._activity())

            asyncio.run(asyncio.wait_for(main(), 60))
        """)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("failed: the provider stopped on an internal error False", result.stdout)
        self.assertIn("then: Yes.", result.stdout)
        self.assertIn("handing: True 0", result.stdout)

    def test_once_no_thread_hands_the_calls_a_call_fails_at_once(self) -> None:
        result = run_child("""
            async def main():
                answering, _ = scripted(responses=[{"content": "Yes."}])
                silent, seen = scripted(silent=True)
                # Two calls under way: one waits for the other to be dropped.
                waiting = seen.dropped(1)
                asking = silent._native.generate(HELLO_JSON, None)
                await seen.begun(1)
                _native._handing_ends()
                print("answered:", (await answering.generate(HELLO)).content)
                loop = asyncio.get_running_loop()
                deadline = loop.time() + 30
                while _native._handing() and loop.time() < deadline:
                    await asyncio.sleep(0.001)
                print("handing:", _native._handing())
                # A call made now would wait for good: it is refused.
                for call in (
                    lambda: answering.generate(HELLO),
                    lambda: answering.list_models(),
                    lambda: anext(answering.generate_structured_stream(HELLO)),
                ):
                    try:
                        await asyncio.wait_for(call(), 30)
                    except RuntimeError as refused:
                        print("refused:", "lost the thread" in str(refused))
                # A call that was under way still ends: the thread that ran
                # it hands it to Python itself.
                asking.cancel()
                await asyncio.wait_for(waiting, 30)
                print("under way: ended")

            asyncio.run(asyncio.wait_for(main(), 100))
            print("exited with", _native._activity())
        """)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("answered: Yes.", result.stdout)
        self.assertIn("handing: False", result.stdout)
        self.assertEqual(result.stdout.count("refused: True"), 3, result.stdout)
        self.assertIn("under way: ended", result.stdout)
        self.assertIn("exited with 0", result.stdout)

    def test_calls_left_with_the_handing_thread_when_it_ends_still_settle(self) -> None:
        result = run_child("""
            async def main():
                provider, _ = scripted(responses=[{"content": "Yes."}])
                assert (await provider.generate(HELLO)).content == "Yes."
                # Four hundred calls end while this thread holds the
                # interpreter: their ends wait with the handing thread,
                # which hands some, then ends.
                pending = [provider._native.generate(HELLO_JSON, None) for _ in range(400)]
                _native._handing_ends()
                answered = await asyncio.wait_for(asyncio.gather(*pending), 60)
                said = {json.loads(answer)["content"] for answer in answered}
                # The answers are here before the thread that handed the
                # last one has let go of its call, and before it has ended.
                loop = asyncio.get_running_loop()
                deadline = loop.time() + 30
                while (_native._handing() or _native._activity()) and loop.time() < deadline:
                    await asyncio.sleep(0.001)
                print("settled:", len(answered), said, "handing:", _native._handing())

            asyncio.run(main())
            print("exited with", _native._activity())
        """)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("settled: 400 {'Yes.'} handing: False", result.stdout)
        self.assertIn("exited with 0", result.stdout)

    def test_a_forked_child_leaves_what_it_inherited_as_it_is(self) -> None:
        result = run_child("""
            import warnings

            # Forking a process with threads is the scenario under test.
            warnings.filterwarnings("ignore", category=DeprecationWarning)

            async def started(held):
                await held.list_models()

            inherited = _native.provider("scripted", "{}", "")
            asyncio.run(started(inherited))
            before = _native._released()
            pid = os.fork()
            if pid == 0:
                # Releasing it could take a lock a thread of the parent held.
                del inherited
                gc.collect()
                os._exit(0 if _native._released() == before else 7)
            _, status = os.waitpid(pid, 0)
            print("child:", os.waitstatus_to_exitcode(status))
            # The count bites: the parent releases what it holds.
            del inherited
            gc.collect()
            print("parent released:", _native._released() - before)
        """)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("child: 0", result.stdout)
        self.assertIn("parent released: 1", result.stdout)


class KeylessTests(unittest.IsolatedAsyncioTestCase):
    """A provider whose key is optional, and one that takes none: each is
    built, answers and fails as a provider with a key, through what every
    provider shares."""

    HELD: dict[str, Any] = {  # noqa: RUF012
        "base_url": f"https://team:{PASSWORD}@llm.example/v1?token={PASSWORD}",
        "headers": {"X-Team-Token": HEADER},
    }

    async def test_a_provider_built_with_no_key_answers(self) -> None:
        for provider in (
            OptionalKey(OptionalKeyConfig(expects_no_key=True)),
            NoKey(NoKeyConfig()),
        ):
            with self.subTest(provider=type(provider).__qualname__):
                self.assertEqual(provider.name, "MockAIProvider")
                self.assertEqual((await provider.generate(HELLO)).content, "Hello from AI")
                events = [event async for event in provider.generate_structured_stream(HELLO)]
                self.assertEqual(events[-1].type, "done")

    async def test_an_optional_key_that_is_given_crosses_as_any_key(self) -> None:
        crossed = []

        def recording(vendor: str, config_json: str, api_key: Any) -> Any:
            crossed.append((json.loads(config_json).get("api_key", "absent"), api_key))
            return build(vendor, config_json, api_key)

        build = _native.provider
        with mock.patch.object(_config._native, "provider", recording):
            OptionalKey(OptionalKeyConfig(api_key=KEY, expects_key=KEY))
            OptionalKey(OptionalKeyConfig(expects_no_key=True))
            NoKey(NoKeyConfig())
        (masked, given), (unset, none), (absent, no_member) = crossed
        self.assertEqual(masked, "**********")
        self.assertIsInstance(given, SecretStr)
        self.assertEqual((unset, none), (None, None))
        self.assertEqual((absent, no_member), ("absent", None))
        # Neither is built when a key crosses that should not, or the other
        # way round.
        with self.assertRaises(ProviderError):
            OptionalKey(OptionalKeyConfig(api_key=KEY, expects_no_key=True))
        with self.assertRaises(ProviderError):
            OptionalKey(OptionalKeyConfig(expects_key=KEY))

    def test_a_configuration_with_no_key_shows_no_secret(self) -> None:
        for config in (OptionalKeyConfig(**self.HELD), NoKeyConfig(**self.HELD)):
            with self.subTest(config=type(config).__qualname__):
                said = repr(config)
                for secret in (PASSWORD, HEADER):
                    self.assertNotIn(secret, said)
                self.assertIn("base_url='https://llm.example'", said)
        self.assertIn("api_key=None", repr(OptionalKeyConfig()))
        self.assertNotIn(KEY, repr(OptionalKeyConfig(api_key=KEY)))

    async def test_a_provider_with_no_key_fails_as_one_with_a_key(self) -> None:
        # One that cannot be built: RoomKit's failure, no secret in a frame.
        for build in (
            lambda: OptionalKey(OptionalKeyConfig(**self.HELD, refuses="no such endpoint")),
            lambda: NoKey(NoKeyConfig(**self.HELD, refuses="no such endpoint")),
        ):
            try:
                build()
            except ProviderError as refused:
                failed, frames = refused, shown(refused)
            else:
                self.fail("the provider was built")
            self.assertEqual((failed.args[0], failed.provider), ("no such endpoint", "scripted"))
            for secret in (PASSWORD, HEADER):
                self.assertNotIn(secret, frames)
        # One whose call fails: the key a context lends is in no failure.
        refusal = {
            "message": f"Incorrect key provided: {ONE_REQUEST_KEY}.",
            "retryable": False,
            "provider": "scripted",
            "status_code": 401,
            "context_overflow": None,
            "before_request": False,
        }
        provider = OptionalKey(OptionalKeyConfig(failure=refusal))
        lent = AIContext(
            messages=[AIMessage(role="user", content="Hello")],
            metadata={"api_key": ONE_REQUEST_KEY},
        )
        for call in (provider.generate(lent), anext(provider.generate_structured_stream(lent))):
            with self.assertRaises(ProviderError) as failed:
                await call
            self.assertEqual(failed.exception.args[0], "Incorrect key provided: [***].")
            self.assertEqual(failed.exception.status_code, 401)

    def test_a_key_a_configuration_states_is_one_that_masks_itself(self) -> None:
        with self.assertRaises(TypeError):

            class PlainKeyConfig(Config, vendor="scripted"):
                api_key: str
                model: str = "mock"

        with self.assertRaises(TypeError):

            class OptionalPlainKeyConfig(Config, vendor="scripted"):
                api_key: str | None = None
                model: str = "mock"


class CutStreamTests(unittest.IsolatedAsyncioTestCase):
    """A call a stream handed before it was cut does not run: the stream's
    failure is its last item, and RoomKit runs no call of a turn that
    failed."""

    CALL = AIToolCall(id="call_lookup", name="lookup_order", arguments={"number": "A-1"})

    def cut(self) -> Provider:
        """A provider whose stream writes a call, hands it whole, then is
        cut before its end."""
        response = AIResponse(content="", tool_calls=[self.CALL], finish_reason="tool_calls")
        script = {
            "streaming": True,
            "tool_call_delta_chunks": 2,
            "cut_after": 3,
            "label": "scripted",
        }
        return scripted(response, **script)[0]

    async def test_a_stream_that_is_cut_hands_what_came_then_fails(self) -> None:
        given = []
        with self.assertRaises(ProviderError) as failed:
            async for event in self.cut().generate_structured_stream(HELLO):
                given.append(event)
        self.assertEqual(
            [event.type for event in given], ["tool_call_delta", "tool_call_delta", "tool_call"]
        )
        self.assertFalse(given[-1].partial)
        self.assertTrue(failed.exception.retryable)

    async def asked_of_the_tool(self, provider: Provider) -> list[tuple[str, dict[str, Any]]]:
        """What the tool's handler was asked in one turn of an AI channel
        over `provider`, a user's message to its end."""
        asked: list[tuple[str, dict[str, Any]]] = []

        async def handle(name: str, arguments: dict[str, Any]) -> str:
            asked.append((name, arguments))
            return json.dumps({"status": "left this morning"})

        async def row(_connection: str, _event: Any) -> None:
            return None

        lookup = AITool(
            name="lookup_order",
            description="Finds an order by its number.",
            parameters={"type": "object", "properties": {"number": {"type": "string"}}},
        )
        kit = RoomKit()
        self.addAsyncCleanup(kit.close)
        assistant = AIChannel(
            "assistant",
            provider=provider,
            tools=[lookup],
            tool_handler=handle,
            # A turn that failed is asked again once.
            retry_policy=RetryPolicy(max_retries=1, base_delay_seconds=0.001),
        )
        user = WebSocketChannel("user")
        user.register_connection("console", row, room_id="orders")
        kit.register_channel(user)
        kit.register_channel(assistant)
        await kit.create_room(room_id="orders")
        await kit.attach_channel("orders", "user")
        await kit.attach_channel("orders", "assistant", category=ChannelCategory.INTELLIGENCE)
        message = InboundMessage(
            channel_id="user", sender_id="customer", content=TextContent(body="Where is A-1?")
        )
        async with asyncio.timeout(30):
            await kit.process_inbound(message, room_id="orders")
        return asked

    async def test_a_call_of_a_stream_that_was_cut_is_not_run_by_an_ai_channel(self) -> None:
        # The same call, from a stream that reached its end, runs.
        called = AIResponse(content="", tool_calls=[self.CALL], finish_reason="tool_calls")
        whole, _ = scripted(called, AIResponse(content="It left this morning."), streaming=True)
        self.assertEqual(await self.asked_of_the_tool(whole), [("lookup_order", {"number": "A-1"})])
        # Handed whole by a stream that then failed, it does not: the turn
        # failed, and was asked again once, to the same end.
        self.assertEqual(await self.asked_of_the_tool(self.cut()), [])


if __name__ == "__main__":
    unittest.main()
