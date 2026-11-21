"""A fake of a provider's endpoint: a local server that answers each
connection with the next reply of a script, and keeps what it was sent.

It listens on a port of its own on this machine: nothing leaves it. A test
waits on what the fake observed (a request received, a connection its
client closed), never on a timer.
"""

from __future__ import annotations

import asyncio
import json
import socket
import threading
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field

# How long a test waits for what must happen before it says the fake never
# observed it. Nothing passes sooner or later because of it.
DEADLINE = 30.0


@dataclass(frozen=True)
class Refused:
    """Reads the request and closes the connection: no answer comes."""


@dataclass(frozen=True)
class Silent:
    """Reads the request and never answers."""


@dataclass(frozen=True)
class Whole:
    """Answers with a body sent whole, its length declared."""

    status: int
    headers: dict[str, str]
    body: bytes
    # Awaited before the answer is written, when the test gates it.
    gate: asyncio.Event | None = None


@dataclass(frozen=True)
class Reads:
    """Answers with a body sent read by read: each read one chunk of the
    transfer, written and flushed before the next. Then the stream ends
    (``ends``), the connection is lost (``lost``), or nothing more comes
    (``silent``)."""

    status: int
    headers: dict[str, str]
    reads: list[bytes]
    then: str = "ends"
    # Awaited before each read after the first, when the test steps them.
    step: Callable[[], Awaitable[None]] | None = None


Reply = Refused | Silent | Whole | Reads


@dataclass
class Seen:
    """A request the fake was sent."""

    method: str
    # The path and the query.
    target: str
    # The headers, their names in lower case: the last of each name.
    headers: dict[str, str]
    # Every header line, in the order sent, its name in lower case.
    lines: list[tuple[str, str]]
    body: bytes
    # How many reads its answer was written in: none for one sent whole.
    reads: int = 0


@dataclass
class _Observed:
    """A count of something the fake observed, which a test waits on."""

    count: int = 0
    waited: int = 0
    changed: asyncio.Condition = field(default_factory=asyncio.Condition)

    async def tell(self) -> None:
        async with self.changed:
            self.count += 1
            self.changed.notify_all()

    async def next(self) -> None:
        """Returns once it was observed one more time than was waited for."""
        async with self.changed:
            self.waited += 1
            await asyncio.wait_for(
                self.changed.wait_for(lambda: self.count >= self.waited), DEADLINE
            )


class Fake:
    """The fake, on the event loop that starts it."""

    def __init__(self) -> None:
        self.seen: list[Seen] = []
        self.port = 0
        self._replies: list[Reply] = []
        self._server: asyncio.Server | None = None
        self._serving: set[asyncio.Task[None]] = set()
        self._received = _Observed()
        self._silent = _Observed()
        self._closed = _Observed()

    async def start(self) -> Fake:
        self._server = await asyncio.start_server(self._accepted, "127.0.0.1", 0)
        self.port = self._server.sockets[0].getsockname()[1]
        return self

    async def stop(self) -> None:
        if self._server is not None:
            self._server.close()
        for task in list(self._serving):
            task.cancel()
        await asyncio.gather(*self._serving, return_exceptions=True)

    async def __aenter__(self) -> Fake:
        return await self.start()

    async def __aexit__(self, *_: object) -> None:
        await self.stop()

    def answer(self, *replies: Reply) -> None:
        """Sets the script: the replies to the next connections, in order.
        A connection past them is answered 599. What was seen is forgotten."""
        self._replies = list(replies)
        self.seen = []

    async def received(self) -> None:
        """Returns once one more request was received."""
        await self._received.next()

    async def silent(self) -> None:
        """Returns once the fake stopped answering one more request and
        keeps its connection open."""
        await self._silent.next()

    async def closed(self) -> None:
        """Returns once the client closed one more connection the fake kept
        open."""
        await self._closed.next()

    async def served(self) -> None:
        """Returns once every connection accepted so far was served to its
        end: its answer written, and its client gone where the fake keeps
        the connection open."""
        serving = list(self._serving)
        if serving:
            await asyncio.wait_for(asyncio.gather(*serving, return_exceptions=True), DEADLINE)

    async def _accepted(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        # Each read goes out as it is written.
        writer.get_extra_info("socket").setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        reply = self._replies.pop(0) if self._replies else Whole(599, {}, b"")
        task = asyncio.current_task()
        assert task is not None
        self._serving.add(task)
        try:
            await self._answer(reader, writer, reply)
        except (ConnectionError, asyncio.IncompleteReadError):
            pass
        finally:
            self._serving.discard(task)
            writer.close()

    async def _answer(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter, reply: Reply
    ) -> None:
        seen = await _read_request(reader)
        if seen is None:
            return
        self.seen.append(seen)
        await self._received.tell()
        if isinstance(reply, Refused):
            return
        if isinstance(reply, Silent):
            return await self._hold(reader)
        head = [f"HTTP/1.1 {reply.status} Status", "connection: close"]
        head += [f"{name}: {value}" for name, value in reply.headers.items()]
        if isinstance(reply, Whole):
            if reply.gate is not None:
                await reply.gate.wait()
            head.append(f"content-length: {len(reply.body)}\r\n\r\n")
            writer.write("\r\n".join(head).encode() + reply.body)
            await writer.drain()
            return
        head.append("transfer-encoding: chunked\r\n\r\n")
        writer.write("\r\n".join(head).encode())
        await writer.drain()
        for read in filter(None, reply.reads):
            if reply.step is not None and seen.reads:
                await reply.step()
            seen.reads += 1
            writer.write(b"%x\r\n%b\r\n" % (len(read), read))
            await writer.drain()
        if reply.then == "ends":
            writer.write(b"0\r\n\r\n")
            await writer.drain()
        elif reply.then == "silent":
            await self._hold(reader)

    async def _hold(self, reader: asyncio.StreamReader) -> None:
        """Keeps a connection open and silent until its client closes it,
        and tells so."""
        await self._silent.tell()
        try:
            while await reader.read(64):
                pass
        except ConnectionError:
            pass
        await self._closed.tell()


async def _read_request(reader: asyncio.StreamReader) -> Seen | None:
    """One request: its head, then the body its ``content-length`` declares."""
    try:
        head = (await reader.readuntil(b"\r\n\r\n")).decode("latin-1")
    except (asyncio.IncompleteReadError, ConnectionError):
        return None
    request_line, *header_lines = head.removesuffix("\r\n\r\n").split("\r\n")
    method, target, _ = request_line.split(" ", 2)
    lines = []
    for line in header_lines:
        name, _, value = line.partition(":")
        lines.append((name.lower(), value.strip()))
    headers = dict(lines)
    body = await reader.readexactly(int(headers.get("content-length", "0")))
    return Seen(method, target, headers, lines, body)


class ThreadedFake:
    """The fake on a thread and an event loop of its own, for a test that
    calls it from several event loops or threads, or without one."""

    def __init__(self) -> None:
        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._loop.run_forever, daemon=True)
        self._thread.start()
        self.fake: Fake = self.run(Fake().start())
        self.port = self.fake.port

    def run[T](self, work: Awaitable[T]) -> T:
        """Runs `work` on the fake's loop, and waits for it."""

        async def awaited() -> T:
            return await work

        return asyncio.run_coroutine_threadsafe(awaited(), self._loop).result(DEADLINE + 5)

    def answer(self, *replies: Reply) -> None:
        async def answer() -> None:
            self.fake.answer(*replies)

        self.run(answer())

    def seen(self) -> list[Seen]:
        async def seen() -> list[Seen]:
            return list(self.fake.seen)

        return self.run(seen())

    def received(self) -> None:
        self.run(self.fake.received())

    def closed(self) -> None:
        self.run(self.fake.closed())

    def stop(self) -> None:
        self.run(self.fake.stop())
        self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(DEADLINE)
        self._loop.close()


def completion(content: str, model: str = "local-model") -> bytes:
    """The body of a whole answer that says `content`."""
    message = {"role": "assistant", "content": content}
    body = {
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 12, "completion_tokens": 5, "total_tokens": 17},
    }
    return json.dumps(body).encode()


def events(*pieces: str, model: str = "local-model") -> list[bytes]:
    """The reads of a streamed answer that says `pieces`, one event a read,
    then its end."""

    def event(delta: dict[str, str], finish: str | None = None) -> bytes:
        choice = {"index": 0, "delta": delta, "finish_reason": finish}
        chunk = {"id": "chatcmpl-1", "model": model, "choices": [choice]}
        return f"data: {json.dumps(chunk)}\n\n".encode()

    reads = [event({"content": piece}) for piece in pieces]
    return [*reads, event({}, "stop"), b"data: [DONE]\n\n"]


JSON = {"content-type": "application/json"}
EVENT_STREAM = {"content-type": "text/event-stream"}


def local_config(port: int) -> dict[str, object]:
    """The members of the configuration of a provider that asks the fake on
    `port`. Its timeout is far past a test's deadline: a provider's own
    timeout never ends a wait in the place of what a test waits for."""
    return {
        "api_key": "placeholder-not-a-key",
        "model": "local-model",
        "base_url": f"http://127.0.0.1:{port}/v1",
        "timeout": 100 * DEADLINE,
    }
