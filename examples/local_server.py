"""Ask a model once whole and once streamed, with the provider alone.

The provider is a RoomKit provider, asked here as RoomKit asks one, with no
room and no channel around it: a context in, RoomKit's models out.

The example talks to a stand-in server it starts itself on a local port:
nothing leaves the machine, and no key is needed. Point `base_url` at a real
OpenAI-compatible server, or leave it out for OpenAI's own endpoint, to ask
a real model.

    python examples/local_server.py
"""

from __future__ import annotations

import asyncio
import json

from roomkit.providers.ai import AIContext, AIMessage, ProviderError, StreamDone, StreamTextDelta
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

ANSWER = ["The order ", "left ", "this morning."]


def whole_answer() -> bytes:
    message = {"role": "assistant", "content": "".join(ANSWER)}
    body = {
        "model": "local-model",
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 9, "completion_tokens": 6, "total_tokens": 15},
    }
    return json.dumps(body).encode()


def streamed_answer() -> list[bytes]:
    def event(delta: dict[str, str], finish: str | None = None) -> bytes:
        choice = {"index": 0, "delta": delta, "finish_reason": finish}
        chunk = {"model": "local-model", "choices": [choice]}
        return f"data: {json.dumps(chunk)}\n\n".encode()

    pieces = [event({"content": piece}) for piece in ANSWER]
    return [*pieces, event({}, "stop"), b"data: [DONE]\n\n"]


async def stand_in(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Answers one Chat Completions request, whole or as a stream."""
    head = (await reader.readuntil(b"\r\n\r\n")).decode("latin-1").lower()
    length = next(line for line in head.split("\r\n") if line.startswith("content-length:"))
    asked = json.loads(await reader.readexactly(int(length.partition(":")[2])))
    if asked.get("stream"):
        writer.write(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n"
            b"connection: close\r\ntransfer-encoding: chunked\r\n\r\n"
        )
        for read in streamed_answer():
            writer.write(b"%x\r\n%b\r\n" % (len(read), read))
            await writer.drain()
        writer.write(b"0\r\n\r\n")
    else:
        body = whole_answer()
        writer.write(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n"
            b"connection: close\r\ncontent-length: %d\r\n\r\n%b" % (len(body), body)
        )
    await writer.drain()
    writer.close()


async def main() -> None:
    server = await asyncio.start_server(stand_in, "127.0.0.1", 0)
    port = server.sockets[0].getsockname()[1]
    config = OpenAIConfig(
        api_key="placeholder-not-a-key",
        model="local-model",
        base_url=f"http://127.0.0.1:{port}/v1",
    )
    provider = OpenAI(config)
    context = AIContext(
        system_prompt="You answer in one sentence.",
        messages=[AIMessage(role="user", content="Where is my order?")],
    )
    try:
        response = await provider.generate(context)
        print("whole:   ", response.content, response.usage)
        assert response.content == "".join(ANSWER), response

        pieces = []
        async for event in provider.generate_structured_stream(context):
            if isinstance(event, StreamTextDelta):
                pieces.append(event.text)
            elif isinstance(event, StreamDone):
                print("streamed:", "".join(pieces), "/", event.finish_reason)
        assert pieces == ANSWER, pieces
    except ProviderError as error:
        raise SystemExit(f"the provider failed: {error}") from error
    finally:
        await provider.close()
        server.close()
        await server.wait_closed()


if __name__ == "__main__":
    asyncio.run(main())
