"""What a turn costs through the package's provider, and through RoomKit's
own Python provider where its SDK is installed.

A turn is asked of a local fake of the provider's wire, which runs in a
process of its own so that its work is not counted: a streamed answer of 500
text deltas sent at once, and a whole answer. Two contexts are asked: 22 kB
of text, and the same with tools and an image. Each variant's turns are
measured in rounds interleaved with the others', so that drift lands on all
of them; the medians of the wall time and of the process's CPU time are
reported, the binding's threads included.

`--provider` names the provider measured, `openai` unless it says
`anthropic`. The variants, each given RoomKit's AIContext and giving
RoomKit's models:

  rust              this package's OpenAI or Anthropic: the Rust provider
  roomkit           RoomKit's OpenAIAIProvider or AnthropicAIProvider: the
                    vendor's SDK, where it is installed

The fake speaks the wire a request's path names: Chat Completions, or
Anthropic's Messages API. The Messages API is asked as a stream by both
variants, for a whole answer too: its two turns read the same 500 deltas
and differ in what the provider hands over, the events or the answer.

Not a test, and not part of any gate. Build the native module optimized
(`maturin develop --release`, which leaves it beside the package's sources),
then run on idle cores:

    taskset -c 0-15 python benchmarks/crossing.py
    taskset -c 0-15 python benchmarks/crossing.py --provider anthropic

and, to compare with RoomKit's provider, from a RoomKit checkout whose
environment has the provider's SDK:

    taskset -c 0-15 uv run --no-sync python ../roomkit-providers/benchmarks/crossing.py
"""

from __future__ import annotations

import argparse
import asyncio
import base64
import importlib
import json
import os
import socket
import statistics
import subprocess
import sys
import time
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DELTAS = 500
ROUNDS = 6
TURNS_PER_ROUND = 25
WARM_UP = 5
MODEL = "local-model"
KEY = "placeholder-not-a-key"
Turn = Callable[[], Awaitable[int]]
# This package's provider, and what builds RoomKit's own.
Pair = tuple[Any, Callable[[], Any]]


# -- The fake of the wire, in a process of its own ------------------------------------


def _event(payload: dict[str, Any] | str, name: str = "") -> bytes:
    """One server-sent event as one chunk of the transfer, under its name
    on a wire that names its events."""
    data = payload if isinstance(payload, str) else json.dumps(payload, separators=(",", ":"))
    named = f"event: {name}\n" if name else ""
    body = f"{named}data: {data}\n\n".encode()
    return b"%x\r\n%b\r\n" % (len(body), body)


def _delta(delta: dict[str, Any], finish: str | None = None) -> dict[str, Any]:
    choice = {"index": 0, "delta": delta, "finish_reason": finish}
    return {"id": "chatcmpl-1", "model": MODEL, "choices": [choice]}


WORDS = [f"word{at} " for at in range(DELTAS)]
USAGE = {"prompt_tokens": 5000, "completion_tokens": DELTAS, "total_tokens": 5000 + DELTAS}
EVENT_STREAM = (
    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n"
)
STREAMED = (
    EVENT_STREAM
    + _event(_delta({"role": "assistant", "content": ""}))
    + b"".join(_event(_delta({"content": word})) for word in WORDS)
    + _event(_delta({}, "stop"))
    + _event({"id": "chatcmpl-1", "model": MODEL, "choices": [], "usage": USAGE})
    + _event("[DONE]")
    + b"0\r\n\r\n"
)
_MESSAGE = {"role": "assistant", "content": "".join(WORDS)}
_COMPLETION = {
    "id": "chatcmpl-1",
    "model": MODEL,
    "choices": [{"index": 0, "message": _MESSAGE, "finish_reason": "stop"}],
    "usage": USAGE,
}
_BODY = json.dumps(_COMPLETION).encode()
WHOLE = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: %d\r\n\r\n%b" % (
    len(_BODY),
    _BODY,
)


def _typed(payload: dict[str, Any]) -> bytes:
    """One event of the Messages API, named by its type."""
    return _event(payload, payload["type"])


def _text_delta(text: str) -> dict[str, Any]:
    delta = {"type": "text_delta", "text": text}
    return {"type": "content_block_delta", "index": 0, "delta": delta}


_STARTED = {
    "id": "msg_1",
    "type": "message",
    "role": "assistant",
    "model": MODEL,
    "content": [],
    "stop_reason": None,
    "stop_sequence": None,
    "usage": {
        "input_tokens": 5000,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
        "output_tokens": 1,
    },
}
_TEXT = {"type": "text", "text": ""}
_ENDED = {"stop_reason": "end_turn", "stop_sequence": None}
# The one answer of the Messages API, which both variants ask as a stream.
MESSAGE = (
    EVENT_STREAM
    + _typed({"type": "message_start", "message": _STARTED})
    + _typed({"type": "content_block_start", "index": 0, "content_block": _TEXT})
    + b"".join(_typed(_text_delta(word)) for word in WORDS)
    + _typed({"type": "content_block_stop", "index": 0})
    + _typed({"type": "message_delta", "delta": _ENDED, "usage": {"output_tokens": DELTAS}})
    + _typed({"type": "message_stop"})
    + b"0\r\n\r\n"
)


async def _serve(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Answers each request of a connection in the wire its path names:
    a message of the Messages API, else a completion, streamed when the
    request asks so."""
    try:
        while True:
            head = (await reader.readuntil(b"\r\n\r\n")).decode("latin-1").lower()
            lengths = [line for line in head.split("\r\n") if line.startswith("content-length:")]
            length = int(lengths[0].partition(":")[2]) if lengths else 0
            asked = json.loads(await reader.readexactly(length)) if length else {}
            if head.split(" ", 2)[1].startswith("/v1/messages"):
                writer.write(MESSAGE)
            else:
                writer.write(STREAMED if asked.get("stream") else WHOLE)
            await writer.drain()
    except (asyncio.IncompleteReadError, ConnectionError):
        pass
    finally:
        writer.close()


async def serve() -> None:
    server = await asyncio.start_server(_serve, "127.0.0.1", 0)
    print("PORT", server.sockets[0].getsockname()[1], flush=True)
    await server.serve_forever()


# -- The contexts ----------------------------------------------------------------------


def text_request() -> dict[str, Any]:
    """A turn's context of ordinary size: a system prompt and 41 text
    messages, about 22 kB."""
    filler = "The quick brown fox jumps over the lazy dog. " * 11
    messages = [
        {"role": "user" if at % 2 == 0 else "assistant", "content": f"{at}: {filler}"}
        for at in range(41)
    ]
    return {"system_prompt": "You are a helpful assistant. " * 20, "messages": messages}


def tools_and_image_request() -> dict[str, Any]:
    """The same context with eight tools and, in its last message, an image
    of 48 kB."""
    request = text_request()
    parameters = {
        "type": "object",
        "properties": {
            "number": {"type": "string", "description": "The order's number."},
            "include": {"type": "array", "items": {"type": "string"}},
            "limit": {"type": "integer", "minimum": 1},
        },
        "required": ["number"],
    }
    request["tools"] = [
        {
            "name": f"lookup_order_{at}",
            "description": "Finds an order by its number, with what it holds. " * 3,
            "parameters": parameters,
        }
        for at in range(8)
    ]
    image = base64.b64encode(bytes(range(256)) * 188).decode()
    request["messages"].append(
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "What does this picture show?"},
                {
                    "type": "image",
                    "url": f"data:image/png;base64,{image}",
                    "mime_type": "image/png",
                },
            ],
        }
    )
    return request


# -- The variants ----------------------------------------------------------------------


def openai_pair(address: str) -> Pair:
    """This package's OpenAI, and what builds RoomKit's own, which needs the
    openai SDK: both ask the fake's Chat Completions."""
    from roomkit.providers.openai import OpenAIAIProvider as RoomKitProvider
    from roomkit.providers.openai import OpenAIConfig as RoomKitConfig
    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

    settings = {"api_key": KEY, "model": MODEL, "base_url": f"{address}/v1"}
    packaged = OpenAI(OpenAIConfig(**settings))
    return packaged, lambda: RoomKitProvider(RoomKitConfig(**settings))


def anthropic_pair(address: str) -> Pair:
    """This package's Anthropic, and what builds RoomKit's own, which needs
    the anthropic SDK: both ask the fake's Messages API."""
    from roomkit.providers.anthropic import AnthropicAIProvider as RoomKitProvider
    from roomkit.providers.anthropic import AnthropicConfig as RoomKitConfig
    from roomkit_providers.llm.anthropic import Anthropic, AnthropicConfig

    settings = {"api_key": KEY, "model": MODEL, "base_url": address}
    packaged = Anthropic(AnthropicConfig(**settings))
    return packaged, lambda: RoomKitProvider(RoomKitConfig(**settings))


# The providers measured, each under the name of the SDK RoomKit's own runs on.
PAIRS: dict[str, Callable[[str], Pair]] = {"openai": openai_pair, "anthropic": anthropic_pair}


def installed(sdk: str) -> bool:
    """Whether the SDK RoomKit's own provider runs on can be imported."""
    try:
        importlib.import_module(sdk)
    except ImportError:
        return False
    return True


def variants_of(vendor: str, address: str, request: dict[str, Any]) -> dict[str, dict[str, Turn]]:
    """The turns of this package's provider and, where the vendor's SDK is
    installed, of RoomKit's own: both take RoomKit's context and give
    RoomKit's models."""
    from roomkit.providers.ai import AIContext, StreamTextDelta

    context = AIContext.model_validate(request)

    def turns(provider: Any) -> dict[str, Turn]:
        async def stream() -> int:
            deltas = 0
            async for event in provider.generate_structured_stream(context):
                deltas += isinstance(event, StreamTextDelta)
            return deltas

        async def whole() -> int:
            return len((await provider.generate(context)).content)

        return {"stream": stream, "whole": whole}

    packaged, roomkit = PAIRS[vendor](address)
    variants = {"rust": turns(packaged)}
    if installed(vendor):
        variants["roomkit"] = turns(roomkit())
    return variants


# -- The measure -----------------------------------------------------------------------


async def measure(turn: Turn, turns: int) -> tuple[list[float], list[float], int]:
    """Wall and process CPU milliseconds of each turn, and what the last one
    returned."""
    wall, cpu, result = [], [], 0
    for _ in range(turns):
        cpu_start, start = time.process_time(), time.perf_counter()
        result = await turn()
        wall.append((time.perf_counter() - start) * 1000)
        cpu.append((time.process_time() - cpu_start) * 1000)
    return wall, cpu, result


async def compare(variants: dict[str, Turn], expected: int) -> dict[str, dict[str, float]]:
    """The variants' turns, interleaved round by round."""
    wall: dict[str, list[float]] = {name: [] for name in variants}
    cpu: dict[str, list[float]] = {name: [] for name in variants}
    for turn in variants.values():
        await measure(turn, WARM_UP)
    for _ in range(ROUNDS):
        for name, turn in variants.items():
            round_wall, round_cpu, result = await measure(turn, TURNS_PER_ROUND)
            assert result == expected, f"{name} gave {result}, not {expected}"
            wall[name] += round_wall
            cpu[name] += round_cpu
    return {
        name: {
            "wall_median_ms": statistics.median(wall[name]),
            "wall_p90_ms": statistics.quantiles(wall[name], n=10)[-1],
            "cpu_median_ms": statistics.median(cpu[name]),
        }
        for name in variants
    }


async def run(vendor: str, address: str) -> dict[str, Any]:
    contexts = {"text": text_request(), "tools and an image": tools_and_image_request()}
    expected = {"stream": DELTAS, "whole": len("".join(WORDS))}
    results: dict[str, Any] = {"provider": vendor, "sdk": installed(vendor), "contexts": {}}
    for label, request in contexts.items():
        variants = variants_of(vendor, address, request)
        measured = {"bytes": len(json.dumps(request))}
        for kind in ("stream", "whole"):
            turns = {name: turn[kind] for name, turn in variants.items()}
            measured[kind] = await compare(turns, expected[kind])
        results["contexts"][label] = measured
    return results


def show(results: dict[str, Any]) -> None:
    turns = ROUNDS * TURNS_PER_ROUND
    vendor = results["provider"]
    print(f"{vendor}: {DELTAS} deltas a streamed turn; {turns} turns a variant; medians, in ms")
    if not results["sdk"]:
        print(f"the {vendor} SDK is not installed: this package's provider is measured alone")
    for label, measured in results["contexts"].items():
        print(f"\ncontext: {label}, {measured['bytes']} bytes as JSON")
        for kind, title in (("stream", "streamed turn"), ("whole", "whole answer")):
            print(f"  {title:<16}{'wall':>9}{'p90':>9}{'CPU':>9}")
            for name, row in measured[kind].items():
                values = (row["wall_median_ms"], row["wall_p90_ms"], row["cpu_median_ms"])
                print(f"    {name:<14}" + "".join(f"{value:>9.2f}" for value in values))
    print("\n" + json.dumps(results))


def routed() -> bool:
    """Whether a route leads off the machine, asked of the kernel: a UDP
    socket connected to an address kept for documentation needs a route
    there, and sends nothing."""
    for family, address in ((socket.AF_INET, "192.0.2.1"), (socket.AF_INET6, "2001:db8::1")):
        try:
            with socket.socket(family, socket.SOCK_DGRAM) as probe:
                probe.connect((address, 9))
        except OSError:
            continue
        return True
    return False


def without_network() -> None:
    """Runs this program again where no network is reachable but loopback
    (scripts/no-network.sh), unless no route leads off the machine already:
    a bench that lost its way to its local fake then reaches no service.

    What decides is what is true, never a variable this program inherits.
    It runs with a route off the machine only where the script could make
    no namespace and said so, under the opt-out the script names."""
    if not routed():
        return
    allowed = os.environ.get("ROOMKIT_PROVIDERS_ALLOW_NETWORK") == "1"
    if allowed and os.environ.get("ROOMKIT_PROVIDERS_NO_NETWORK") == "unavailable":
        return
    wrapper = str(ROOT / "scripts/no-network.sh")
    os.execv(wrapper, [wrapper, sys.executable, *sys.argv])


def main() -> None:
    without_network()
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--serve", action="store_true", help="run the fake of the wire")
    parser.add_argument(
        "--provider", choices=list(PAIRS), default="openai", help="the provider measured"
    )
    arguments = parser.parse_args()
    if arguments.serve:
        asyncio.run(serve())
        return
    try:
        import roomkit_providers  # noqa: F401
    except ImportError:
        # The module a local build left beside the package's sources.
        sys.path.insert(0, str(ROOT / "python"))
    server = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve()), "--serve"],
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        assert server.stdout is not None
        port = server.stdout.readline().split()[1]
        results = asyncio.run(run(arguments.provider, f"http://127.0.0.1:{port}"))
    finally:
        server.terminate()
        server.wait()
    show(results)


if __name__ == "__main__":
    main()
