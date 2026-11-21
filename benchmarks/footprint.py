"""What a busy provider holds: its threads, its idle connections, and how
fast it serves many streams at once.

A provider is asked of a local fake of the wire, which runs in a process of
its own:

  threads           the process's threads while 200 streams are read at once
  connections       the sockets the process holds once a burst of whole
                    answers is over, then some seconds later (two of them
                    are the event loop's own)
  bursts            the time 200 streamed turns asked at once take

The variants are `benchmarks/crossing.py`'s: this package's provider and,
where the openai SDK is installed, RoomKit's own.

Not a test, and not part of any gate. Build the native module optimized
(`maturin develop --release`), then:

    python benchmarks/footprint.py
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import resource
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from crossing import without_network

ROOT = Path(__file__).resolve().parents[1]
STREAMS = 200
ROUNDS = 7


def threads() -> int:
    return len(os.listdir("/proc/self/task"))


def sockets() -> int:
    held = 0
    for name in os.listdir("/proc/self/fd"):
        try:
            held += os.readlink(f"/proc/self/fd/{name}").startswith("socket:")
        except OSError:
            continue
    return held


def providers(base_url: str) -> dict[str, Any]:
    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

    settings = {"api_key": "placeholder-not-a-key", "model": "m", "base_url": base_url}
    found = {"rust": OpenAI(OpenAIConfig(**settings))}
    try:
        import openai  # noqa: F401
    except ImportError:
        return found
    from roomkit.providers.openai import OpenAIAIProvider
    from roomkit.providers.openai import OpenAIConfig as RoomKitConfig

    found["roomkit"] = OpenAIAIProvider(RoomKitConfig(**settings))
    return found


async def measure(provider: Any, burst: int, wait: float) -> dict[str, Any]:
    from roomkit.providers.ai import AIContext, AIMessage

    hello = AIContext(messages=[AIMessage(role="user", content="Hello")])

    async def stream() -> None:
        async for _ in provider.generate_structured_stream(hello):
            pass

    async def streams() -> float:
        started = time.perf_counter()
        await asyncio.gather(*(stream() for _ in range(STREAMS)))
        return (time.perf_counter() - started) * 1000

    await streams()
    seen: list[int] = []

    async def watch() -> None:
        while True:
            seen.append(threads())
            await asyncio.sleep(0.005)

    watching = asyncio.create_task(watch())
    took = [await streams() for _ in range(ROUNDS)]
    watching.cancel()
    await asyncio.gather(*(provider.generate(hello) for _ in range(burst)))
    held = {"at its end": sockets()}
    await asyncio.sleep(wait)
    held[f"{wait:g} s later"] = sockets()
    return {
        "threads": {"least": min(seen), "most": max(seen), "idle": threads()},
        "connections after a burst": held,
        "burst of streams, ms": {"median": statistics.median(took), "slowest": max(took)},
    }


async def run(base_url: str, burst: int, wait: float) -> dict[str, Any]:
    return {
        name: await measure(provider, burst, wait) for name, provider in providers(base_url).items()
    }


def main() -> None:
    without_network()
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--burst", type=int, default=2100, help="whole answers asked at once")
    parser.add_argument("--wait", type=float, default=10.0, help="seconds before the recount")
    options = parser.parse_args()
    try:
        import roomkit_providers  # noqa: F401
    except ImportError:
        # The module a local build left beside the package's sources.
        sys.path.insert(0, str(ROOT / "python"))
    # A burst holds one socket a call, here and in the fake.
    _, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    resource.setrlimit(resource.RLIMIT_NOFILE, (hard, hard))
    server = subprocess.Popen(
        [sys.executable, str(Path(__file__).with_name("crossing.py")), "--serve"],
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        assert server.stdout is not None
        port = server.stdout.readline().split()[1]
        results = asyncio.run(run(f"http://127.0.0.1:{port}/v1", options.burst, options.wait))
    finally:
        server.terminate()
        server.wait()
    print(f"{STREAMS} streams at once, {ROUNDS} rounds; a burst of {options.burst} whole answers")
    for name, measured in results.items():
        print(f"\n{name}")
        for what, values in measured.items():
            print(f"  {what}: {values}")
    print("\n" + json.dumps(results))


if __name__ == "__main__":
    main()
