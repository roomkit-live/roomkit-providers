"""A RoomKit agent over the adapter: a kit whose AI channel asks a local
fake of the endpoint through the Rust provider. Everything is observed
through RoomKit: what its channels deliver, what its tool handler is asked,
what its telemetry records."""

from __future__ import annotations

import asyncio
import json
import unittest

import adapter_tests  # noqa: F401
from fake_endpoint import DEADLINE, EVENT_STREAM, Fake, Reads, call_events, events, local_config
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
from roomkit.providers.ai import AITool
from roomkit.providers.openai import OpenAIConfig
from roomkit.telemetry import MockTelemetryProvider
from roomkit_providers.roomkit import OpenAIAIProvider

ORDER = AITool(
    name="lookup_order",
    description="Finds an order by its number.",
    parameters={
        "type": "object",
        "properties": {"number": {"type": "string"}},
        "required": ["number"],
    },
)


class Agent:
    """A room with a user on a WebSocket channel and an AI channel over the
    adapter, and what the user's connection was delivered."""

    def __init__(self, fake: Fake, *, streaming: bool, telemetry=None) -> None:
        self.kit = RoomKit(telemetry=telemetry)
        self.provider = OpenAIAIProvider(OpenAIConfig(**local_config(fake.port)))
        self.asked: list[tuple[str, dict]] = []
        self.rows: list[str] = []
        self.stream: list[object] = []
        self.calls: list[object] = []
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

        async def row(_connection: str, event) -> None:
            if isinstance(event.content, TextContent):
                self.rows.append(event.content.body)

        async def streamed(_connection: str, message) -> None:
            self.stream.append(message)

        @kit.hook(HookTrigger.ON_TOOL_CALL, execution=HookExecution.ASYNC)
        async def observe(event, _context) -> None:
            self.calls.append(event)

        stream_send = streamed if self.streaming else None
        user.register_connection("console", row, room_id="orders", stream_send_fn=stream_send)
        kit.register_channel(user)
        kit.register_channel(assistant)
        await kit.create_room(room_id="orders")
        await kit.attach_channel("orders", "user")
        await kit.attach_channel("orders", "assistant", category=ChannelCategory.INTELLIGENCE)

    async def says(self, text: str) -> None:
        """The user says `text`: the turn it starts is over when this returns."""
        message = InboundMessage(
            channel_id="user", sender_id="customer", content=TextContent(body=text)
        )
        await self.kit.process_inbound(message, room_id="orders")


class AgentTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)

    async def agent(self, **settings) -> Agent:
        agent = Agent(self.fake, **settings)
        self.addAsyncCleanup(agent.kit.close)
        await agent.open()
        return agent

    def bodies(self) -> list[dict]:
        return [json.loads(seen.body) for seen in self.fake.seen]

    async def test_an_agent_answers_a_message(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("We open ", "at nine.")))
        agent = await self.agent(streaming=False)
        async with asyncio.timeout(DEADLINE):
            await agent.says("When do you open?")
        # The room delivered the answer to the user's connection, whole.
        self.assertEqual(agent.rows, ["We open at nine."])
        (asked,) = self.bodies()
        system, *_, last = asked["messages"]
        self.assertEqual(system["role"], "system")
        self.assertIn("You answer in one sentence.", system["content"])
        self.assertEqual(last, {"role": "user", "content": "When do you open?"})
        # The channel's tools, RoomKit's own among them, were declared.
        self.assertIn("lookup_order", [tool["function"]["name"] for tool in asked["tools"]])
        self.assertTrue(asked["stream"])

    async def test_an_agent_calls_a_tool_and_answers_with_its_result(self) -> None:
        self.fake.answer(
            Reads(200, EVENT_STREAM, call_events("lookup_order", '{"number": "A-1"}')),
            Reads(200, EVENT_STREAM, events("Your order ", "left this morning.")),
        )
        agent = await self.agent(streaming=False)
        async with asyncio.timeout(DEADLINE):
            await agent.says("Where is my order A-1?")
        # RoomKit ran the call the model made, and told its hook.
        self.assertEqual(agent.asked, [("lookup_order", {"number": "A-1"})])
        self.assertEqual(
            [(call.name, call.is_error) for call in agent.calls], [("lookup_order", False)]
        )
        # The second round showed the model its call and what the tool answered.
        first, second = self.bodies()
        self.assertEqual(len(second["messages"]), len(first["messages"]) + 2)
        called, answered = second["messages"][-2:]
        self.assertEqual(called["role"], "assistant")
        self.assertEqual(called["tool_calls"][0]["function"]["name"], "lookup_order")
        self.assertEqual(answered["role"], "tool")
        self.assertEqual(answered["tool_call_id"], called["tool_calls"][0]["id"])
        self.assertEqual(json.loads(answered["content"])["status"], "left this morning")
        self.assertEqual(agent.rows, ["Your order left this morning."])

    async def test_an_agent_streams_its_answer_to_a_channel_that_takes_streams(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("We ", "open ", "at nine.")))
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
            Reads(200, EVENT_STREAM, call_events("lookup_order", '{"number": "A-1"}')),
            Reads(200, EVENT_STREAM, events("Your order ", "left ", "this morning.")),
        )
        agent = await self.agent(streaming=True, telemetry=telemetry)
        async with asyncio.timeout(DEADLINE):
            await agent.says("Where is my order A-1?")
        # Two generations: the first wrote a call alone, which is no token
        # of an answer; the second wrote text, three pieces of it.
        recorded = [
            metric for metric in telemetry.metrics if metric["name"] == "roomkit.llm.ttfb_ms"
        ]
        self.assertEqual(len(recorded), 1, telemetry.metrics)
        (metric,) = recorded
        self.assertEqual(metric["unit"], "ms")
        self.assertEqual(
            metric["attributes"], {"provider": "OpenAIAIProvider", "model": "local-model"}
        )
        self.assertGreaterEqual(metric["value"], 0)


if __name__ == "__main__":
    unittest.main()
