"""A RoomKit agent whose model is asked through the Rust provider of
Anthropic's Messages API.

The provider takes the place of RoomKit's own Anthropic provider: it is given
to an AI channel as any provider is. Here the model reasons before it calls a
tool. The agent runs the tool and sends the round back, the reasoning as it
came, with its signature, which the API requires; then it streams the answer
to the customer's connection.

The example talks to a stand-in server it starts itself on a local port:
nothing leaves the machine, and no key is needed.

    python examples/anthropic_agent.py
"""

from __future__ import annotations

import asyncio
import json
from collections.abc import Awaitable, Callable
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
from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

REASONING = "The customer asks where order A-1 is."
# What signs a block of reasoning: opaque to everyone but the API.
SIGNATURE = "c2lnbmF0dXJlLW9uZQ=="
ANSWER = ["Your order ", "left ", "this morning."]

# A content block of a streamed message: what its start holds, and its deltas.
Block = tuple[dict[str, Any], list[dict[str, Any]]]


def event(data: dict[str, Any]) -> bytes:
    return f"event: {data['type']}\ndata: {json.dumps(data)}\n\n".encode()


def message(blocks: list[Block], stop: str) -> list[bytes]:
    """The events of a streamed message: its start, each block with its
    deltas, then why it stopped."""
    started = {
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "local-model",
        "content": [],
        "stop_reason": None,
        "stop_sequence": None,
        "usage": {"input_tokens": 12, "output_tokens": 1},
    }
    events = [event({"type": "message_start", "message": started})]
    for index, (opens, deltas) in enumerate(blocks):
        events.append(
            event({"type": "content_block_start", "index": index, "content_block": opens})
        )
        events += [
            event({"type": "content_block_delta", "index": index, "delta": delta})
            for delta in deltas
        ]
        events.append(event({"type": "content_block_stop", "index": index}))
    ended = {"stop_reason": stop, "stop_sequence": None}
    events.append(event({"type": "message_delta", "delta": ended, "usage": {"output_tokens": 5}}))
    return [*events, event({"type": "message_stop"})]


def reasons_then_calls_the_tool() -> list[bytes]:
    reasoning: Block = (
        {"type": "thinking", "thinking": "", "signature": ""},
        [
            {"type": "thinking_delta", "thinking": REASONING},
            {"type": "signature_delta", "signature": SIGNATURE},
        ],
    )
    call: Block = (
        {"type": "tool_use", "id": "toolu_1", "name": "lookup_order", "input": {}},
        [{"type": "input_json_delta", "partial_json": '{"number": "A-1"}'}],
    )
    return message([reasoning, call], "tool_use")


def answers() -> list[bytes]:
    said: Block = (
        {"type": "text", "text": ""},
        [{"type": "text_delta", "text": piece} for piece in ANSWER],
    )
    return message([said], "end_turn")


def holds_a_result(asked: dict[str, Any]) -> bool:
    """Whether a request shows the model what a tool answered."""
    return any(
        isinstance(block, dict) and block.get("type") == "tool_result"
        for turn in asked["messages"]
        for block in turn["content"]
    )


def stand_in(
    seen: list[dict[str, Any]],
) -> Callable[[asyncio.StreamReader, asyncio.StreamWriter], Awaitable[None]]:
    """What answers a streamed Messages request, and keeps it in `seen`: the
    model reasons and calls the tool first, then, once the request shows the
    tool's result, answers."""

    async def answer(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        head = (await reader.readuntil(b"\r\n\r\n")).decode("latin-1").lower()
        length = next(line for line in head.split("\r\n") if line.startswith("content-length:"))
        asked = json.loads(await reader.readexactly(int(length.partition(":")[2])))
        seen.append(asked)
        writer.write(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n"
            b"connection: close\r\ntransfer-encoding: chunked\r\n\r\n"
        )
        for read in answers() if holds_a_result(asked) else reasons_then_calls_the_tool():
            writer.write(b"%x\r\n%b\r\n" % (len(read), read))
            await writer.drain()
        writer.write(b"0\r\n\r\n")
        await writer.drain()
        writer.close()

    return answer


async def lookup_order(name: str, arguments: dict[str, Any]) -> str:
    """The agent's tool: what the room's host knows of an order."""
    print(f"tool:      {name}({arguments})")
    return json.dumps({"number": arguments["number"], "status": "left this morning"})


async def main() -> None:
    seen: list[dict[str, Any]] = []
    server = await asyncio.start_server(stand_in(seen), "127.0.0.1", 0)
    port = server.sockets[0].getsockname()[1]
    # The base URL is the server's: the provider adds the path of the API.
    config = AnthropicConfig(
        api_key="placeholder-not-a-key",
        model="local-model",
        base_url=f"http://127.0.0.1:{port}",
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
            provider=Anthropic(config),
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
    print("streamed: ", streamed)
    print("answer:   ", "".join(streamed))
    # The room streamed the answer as it came, then stored it whole.
    assert "".join(streamed) == "".join(ANSWER), streamed
    assert "".join(ANSWER) in stored, stored
    # The second request showed the model its own round: its reasoning as it
    # came, with its signature, then its call; and what the tool answered.
    assert len(seen) == 2, len(seen)
    round_, result = seen[1]["messages"][-2:]
    reasoning, call = round_["content"]
    print("sent back:", [reasoning["type"], call["type"]], "then", result["content"][0]["type"])
    assert (reasoning["thinking"], reasoning["signature"]) == (REASONING, SIGNATURE), reasoning
    assert (call["type"], call["id"]) == ("tool_use", "toolu_1"), call
    assert result["content"][0]["tool_use_id"] == "toolu_1", result
    assert "left this morning" in result["content"][0]["content"], result


if __name__ == "__main__":
    asyncio.run(main())
