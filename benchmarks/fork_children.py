"""How often a child forked while calls are under way hangs.

A parent keeps one provider busy against a local fake of the wire, on many
event loops at once and with many streams open, and forks children while it
does. Each child leaves one way, then exits as any Python program does:

  exit              it does nothing
  call              it makes a call, which the package refuses
  drop              it lets go of a provider it inherited
  activity          it reads the native module's count of calls under way

A child that has not ended after its time is counted as hung, and killed.
One that ends otherwise than it should is counted as failed.

Not a test, and not part of any gate. Build the native module (`maturin
develop`), then:

    python benchmarks/fork_children.py --children 3000
"""

from __future__ import annotations

import argparse
import asyncio
import gc
import json
import os
import signal
import subprocess
import sys
import threading
import time
import warnings
from collections.abc import Callable
from pathlib import Path
from typing import Any

from crossing import without_network

ROOT = Path(__file__).resolve().parents[1]
LOOPS = 64
STREAMS = 32
AT_ONCE = 16
PATIENCE = 10.0
REFUSED = 0
NOT_REFUSED = 3


def provider_at(base_url: str) -> Any:
    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

    return OpenAI(OpenAIConfig(api_key="placeholder-not-a-key", model="m", base_url=base_url))


def busy(provider: Any, stop: threading.Event) -> list[threading.Thread]:
    """The parent's load: whole answers on LOOPS event loops, each on a
    thread of its own, and STREAMS streams read on one more."""
    from roomkit.providers.ai import AIContext, AIMessage

    hello = AIContext(messages=[AIMessage(role="user", content="Hello")])

    async def calls() -> None:
        while not stop.is_set():
            await provider.generate(hello)

    async def stream() -> None:
        while not stop.is_set():
            async for _ in provider.generate_structured_stream(hello):
                pass

    async def streams() -> None:
        await asyncio.gather(*(stream() for _ in range(STREAMS)))

    threads = [threading.Thread(target=asyncio.run, args=(calls(),), daemon=True)]
    threads += [
        threading.Thread(target=asyncio.run, args=(calls(),), daemon=True) for _ in range(LOOPS - 1)
    ]
    threads.append(threading.Thread(target=asyncio.run, args=(streams(),), daemon=True))
    for thread in threads:
        thread.start()
    return threads


def calls_once(provider: Any) -> int:
    from roomkit.providers.ai import AIContext, AIMessage

    hello = AIContext(messages=[AIMessage(role="user", content="Hello")])
    try:
        asyncio.run(provider.generate(hello))
    except RuntimeError:
        return REFUSED
    return NOT_REFUSED


def reads_activity(_provider: Any) -> int:
    from roomkit_providers import _native

    return REFUSED if _native._activity() == 0 else NOT_REFUSED


WAYS: dict[str, Callable[[Any], int]] = {
    "exit": lambda _provider: REFUSED,
    "call": calls_once,
    "drop": lambda _provider: REFUSED,
    "activity": reads_activity,
}


def child(way: str, provider: Any, spare: Any) -> None:
    """What a child does, then its exit, as any Python program's."""
    code = WAYS[way](provider)
    if way == "drop":
        del spare
        gc.collect()
    sys.exit(code)


def fork_children(way: str, count: int, provider: Any, base_url: str) -> dict[str, int]:
    """Forks `count` children that leave `way`, AT_ONCE at a time."""
    waited: dict[int, float] = {}
    counts = {"children": 0, "hung": 0, "failed": 0}

    def reap(block: bool) -> None:
        while waited and (block or len(waited) >= AT_ONCE):
            for pid, deadline in list(waited.items()):
                ended, status = os.waitpid(pid, os.WNOHANG)
                if ended:
                    del waited[pid]
                    counts["failed"] += os.waitstatus_to_exitcode(status) != REFUSED
                elif time.monotonic() > deadline:
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
                    del waited[pid]
                    counts["hung"] += 1
            time.sleep(0.001)

    for _ in range(count):
        # A provider nothing else holds, for a child to let go of.
        spare = provider_at(base_url) if way == "drop" else None
        pid = os.fork()
        if pid == 0:
            child(way, provider, spare)
        waited[pid] = time.monotonic() + PATIENCE
        counts["children"] += 1
        reap(block=False)
    reap(block=True)
    return counts


def main() -> None:
    without_network()
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--children", type=int, default=3000, help="children per way of leaving")
    parser.add_argument("--ways", nargs="*", default=list(WAYS), choices=list(WAYS))
    options = parser.parse_args()
    try:
        import roomkit_providers  # noqa: F401
    except ImportError:
        # The module a local build left beside the package's sources.
        sys.path.insert(0, str(ROOT / "python"))
    # The harness forks a process that has threads: that is what it measures.
    warnings.filterwarnings("ignore", category=DeprecationWarning, message=".*fork.*")
    parent = os.getpid()
    server = subprocess.Popen(
        [sys.executable, str(Path(__file__).with_name("crossing.py")), "--serve"],
        stdout=subprocess.PIPE,
        text=True,
    )
    stop = threading.Event()
    threads: list[threading.Thread] = []
    results: dict[str, dict[str, int]] = {}
    try:
        assert server.stdout is not None
        base_url = f"http://127.0.0.1:{server.stdout.readline().split()[1]}/v1"
        provider = provider_at(base_url)
        threads = busy(provider, stop)
        time.sleep(0.5)
        for way in options.ways:
            results[way] = fork_children(way, options.children, provider, base_url)
            print(way, results[way], flush=True)
    finally:
        # A child that exits passes here too, and has nothing to stop.
        if os.getpid() == parent:
            # The calls under way end before what answers them does.
            stop.set()
            for thread in threads:
                thread.join(PATIENCE)
            server.terminate()
            server.wait()
    print(json.dumps({"loops": LOOPS, "streams": STREAMS, "ways": results}))


if __name__ == "__main__":
    main()
