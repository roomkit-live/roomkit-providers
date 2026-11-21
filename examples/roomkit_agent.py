"""A RoomKit agent whose model is asked through the Rust provider.

The adapter takes the place of RoomKit's own OpenAI provider: it is given to
an AI channel as any provider is. Here the agent answers a customer, calls a
tool on the way, and streams its answer to the customer's connection.

The example talks to a stand-in server it starts itself on a local port:
nothing leaves the machine, and no key is needed. It needs RoomKit, which
the `roomkit` extra installs:

    pip install "roomkit-providers[roomkit]"
    python examples/roomkit_agent.py
"""

from __future__ import annotations

import asyncio
import json
from typing import Any

from roomkit import (
    AIChannel,
    ChannelCategory,
    InboundMessage,
    RoomEvent,
    RoomKit,
    TextContent,
    WebSocketChannel,
)
from roomkit.channels import StreamChunk, StreamMessage
from roomkit.providers.ai import AITool
from roomkit.providers.openai import OpenAIConfig
from roomkit_providers.roomkit import OpenAIAIProvider

ANSWER = ["Your order ", "left ", "this morning."]


def event(delta: dict[str, Any], finish: str | None = None) -> bytes:
    choice = {"index": 0, "delta": delta, "finish_reason": finish}
    chunk = {"model": "local-model", "choices": [choice]}
    return f"data: {json.dumps(chunk)}\n\n".encode()


def calls_the_tool() -> list[bytes]:
    function = {"name": "lookup_order", "arguments": '{"number": "A-1"}'}
    call = {"index": 0, "id": "call_1", "type": "function", "function": function}
    return [event({"tool_calls": [call]}), event({}, "tool_calls"), b"data: [DONE]\n\n"]


def answers() -> list[bytes]:
    pieces = [event({"content": piece}) for piece in ANSWER]
    return [*pieces, event({}, "stop"), b"data: [DONE]\n\n"]


async def stand_in(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Answers one streamed Chat Completions request: a call to the tool
    first, then, once the request shows the tool's result, the answer."""
    head = (await reader.readuntil(b"\r\n\r\n")).decode("latin-1").lower()
    length = next(line for line in head.split("\r\n") if line.startswith("content-length:"))
    asked = json.loads(await reader.readexactly(int(length.partition(":")[2])))
    told = any(message["role"] == "tool" for message in asked["messages"])
    writer.write(
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n"
        b"connection: close\r\ntransfer-encoding: chunked\r\n\r\n"
    )
    for read in answers() if told else calls_the_tool():
        writer.write(b"%x\r\n%b\r\n" % (len(read), read))
        await writer.drain()
    writer.write(b"0\r\n\r\n")
    await writer.drain()
    writer.close()


async def lookup_order(name: str, arguments: dict[str, Any]) -> str:
    """The agent's tool: what the room's host knows of an order."""
    print(f"tool:     {name}({arguments})")
    return json.dumps({"number": arguments["number"], "status": "left this morning"})


async def main() -> None:
    server = await asyncio.start_server(stand_in, "127.0.0.1", 0)
    port = server.sockets[0].getsockname()[1]
    config = OpenAIConfig(
        api_key="placeholder-not-a-key",
        model="local-model",
        base_url=f"http://127.0.0.1:{port}/v1",
    )
    order = AITool(
        name="lookup_order",
        description="Finds an order by its number.",
        parameters={
            "type": "object",
            "properties": {"number": {"type": "string"}},
            "required": ["number"],
        },
    )
    streamed: list[str] = []
    stored: list[str] = []

    async def on_stream(_connection: str, message: StreamMessage) -> None:
        if isinstance(message, StreamChunk):
            streamed.append(message.delta)

    async def on_event(_connection: str, event: RoomEvent) -> None:
        if isinstance(event.content, TextContent):
            stored.append(event.content.body)

    kit = RoomKit()
    try:
        assistant = AIChannel(
            "assistant",
            provider=OpenAIAIProvider(config),
            system_prompt="You answer in one sentence.",
            tools=[order],
            tool_handler=lookup_order,
        )
        customer = WebSocketChannel("customer")
        customer.register_connection(
            "console", on_event, room_id="orders", stream_send_fn=on_stream
        )
        kit.register_channel(customer)
        kit.register_channel(assistant)
        await kit.create_room(room_id="orders")
        await kit.attach_channel("orders", "customer")
        await kit.attach_channel("orders", "assistant", category=ChannelCategory.INTELLIGENCE)
        await kit.process_inbound(
            InboundMessage(
                channel_id="customer",
                sender_id="customer",
                content=TextContent(body="Where is my order A-1?"),
            ),
            room_id="orders",
        )
    finally:
        await kit.close()
        server.close()
        await server.wait_closed()
    print("streamed:", streamed)
    print("answer:  ", "".join(streamed))
    # The room streamed the answer as it came, then stored it whole.
    assert "".join(streamed) == "".join(ANSWER), streamed
    assert "".join(ANSWER) in stored, stored


if __name__ == "__main__":
    asyncio.run(main())
