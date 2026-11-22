"""A fake of a provider's endpoint: a local server that answers each
connection with the next reply of a script, and keeps what it was sent.

It listens on a port of its own on this machine: nothing leaves it. A test
waits on what the fake observed (a request received, a connection its
client closed), never on a timer.

It speaks plain HTTP, or TLS with the certificate of an authority a test
makes for itself (``Authority``). It stands for a proxy as it stands for an
endpoint, since a proxy is asked as a server is: it answers a request
handed to it whole, and carries a tunnel (``Tunnel``).
"""

from __future__ import annotations

import asyncio
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from pathlib import Path

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


@dataclass(frozen=True)
class Tunnel:
    """Answers a ``CONNECT`` with a 200, then carries what the client sends
    to the local server on ``port``, and what that server sends back: the
    tunnel of a proxy, whatever host the client asked it for."""

    port: int


Reply = Refused | Silent | Whole | Reads | Tunnel


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
    # How many reads of its answer were written, each counted once it is
    # written: none for an answer sent whole.
    reads: int = 0
    # Whether its client left before the reads of its answer were all
    # written. A client that leaves while they are being written is seen
    # leaving or not, as the two happen to fall; one that stays until the
    # last is written never is.
    left: bool = False


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
    """The fake, on the event loop that starts it. Given `tls`, the
    configuration of a server (``Authority.serving``), it greets each
    connection over TLS before it reads its request; a connection whose
    client leaves the greeting takes no reply of the script."""

    def __init__(self, tls: ssl.SSLContext | None = None) -> None:
        self.seen: list[Seen] = []
        self.port = 0
        self._tls = tls
        self._loop: asyncio.AbstractEventLoop | None = None
        self._told: Callable[[asyncio.AbstractEventLoop, dict[str, object]], object] | None = None
        self._replies: list[Reply] = []
        self._server: asyncio.Server | None = None
        self._serving: set[asyncio.Task[None]] = set()
        self._received = _Observed()
        self._silent = _Observed()
        self._closed = _Observed()

    async def start(self) -> Fake:
        self._server = await asyncio.start_server(self._accepted, "127.0.0.1", 0, ssl=self._tls)
        self.port = self._server.sockets[0].getsockname()[1]
        if self._tls is not None:
            self._loop = asyncio.get_running_loop()
            self._told = self._loop.get_exception_handler()
            self._loop.set_exception_handler(self._left_the_greeting)
        return self

    def _left_the_greeting(
        self, loop: asyncio.AbstractEventLoop, context: dict[str, object]
    ) -> None:
        """A client that leaves the TLS greeting, as one that refuses the
        fake's certificate does, is no error of the fake: a loop that is
        asked to say everything would report it. Anything else is told as
        it was."""
        if isinstance(context.get("exception"), ssl.SSLError):
            return
        if self._told is None:
            loop.default_exception_handler(context)
        else:
            self._told(loop, context)

    async def stop(self) -> None:
        if self._loop is not None:
            self._loop.set_exception_handler(self._told)
            self._loop = None
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
        if isinstance(reply, Tunnel):
            return await _carry(reader, writer, reply.port)
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
        try:
            await _write_reads(writer, "\r\n".join(head).encode(), reply, seen)
        except ConnectionError:
            seen.left = True
            raise
        # The reads written, the client has the answer: one that leaves
        # before the transfer is ended has left nothing of it.
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


async def _write_reads(writer: asyncio.StreamWriter, head: bytes, reply: Reads, seen: Seen) -> None:
    """Writes the head of an answer sent read by read, then each read as
    one chunk of the transfer, flushed before the next and counted in `seen`
    once it is written. It fails as the connection does where the client
    left before the last read was written."""
    writer.write(head)
    await writer.drain()
    for read in filter(None, reply.reads):
        if reply.step is not None and seen.reads:
            await reply.step()
        writer.write(b"%x\r\n%b\r\n" % (len(read), read))
        await writer.drain()
        seen.reads += 1


async def _carry(reader: asyncio.StreamReader, writer: asyncio.StreamWriter, port: int) -> None:
    """Answers a ``CONNECT`` with a 200, then carries the bytes between the
    client and the local server on `port` until one of them ends."""
    server_reader, server_writer = await asyncio.open_connection("127.0.0.1", port)

    async def passed(source: asyncio.StreamReader, sink: asyncio.StreamWriter) -> None:
        try:
            while read := await source.read(65536):
                sink.write(read)
                await sink.drain()
        except ConnectionError:
            pass
        finally:
            sink.close()

    writer.write(b"HTTP/1.1 200 Connection established\r\n\r\n")
    await writer.drain()
    try:
        await asyncio.gather(passed(reader, server_writer), passed(server_reader, writer))
    finally:
        server_writer.close()


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


JSON = {"content-type": "application/json"}
EVENT_STREAM = {"content-type": "text/event-stream"}

# The program that makes the certificates of a test: none where the machine
# has it not, and the tests that need one are skipped, saying so.
OPENSSL = shutil.which("openssl")
NO_OPENSSL = "the certificates of this test are made with the openssl program, which is not here"


class Authority:
    """A certificate authority made for a test with the ``openssl`` program,
    in a directory of its own. Its key is made when the test runs and
    removed with the directory (``close``): none is kept anywhere else."""

    def __init__(self) -> None:
        self._held = tempfile.TemporaryDirectory(prefix="roomkit-providers-authority-")
        self.directory = Path(self._held.name)
        self._made = 0
        # The file a client is told to trust: the authority's certificate.
        self.pem = self.directory / "authority.pem"
        self._run(
            *("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes"),
            *("-keyout", "authority.key", "-out", "authority.pem", "-days", "2"),
            # A name of its own, so that a client tells two authorities apart.
            *("-subj", f"/CN=authority {self.directory.name[-8:]} of a test"),
            *("-addext", "basicConstraints=critical,CA:TRUE"),
            *("-addext", "keyUsage=critical,keyCertSign,cRLSign"),
        )

    def _run(self, *arguments: str) -> str:
        assert OPENSSL is not None, NO_OPENSSL
        ran = subprocess.run(
            [OPENSSL, *arguments], cwd=self.directory, check=True, capture_output=True, text=True
        )
        return ran.stdout

    def serving(self, *names: str) -> ssl.SSLContext:
        """The TLS of a server whose certificate this authority signed for
        `names`, each a host name or an IPv4 address."""
        self._made += 1
        made = f"server-{self._made}"
        kinds = ("IP" if name.replace(".", "").isdigit() else "DNS" for name in names)
        named = ",".join(f"{kind}:{name}" for kind, name in zip(kinds, names, strict=True))
        extensions = f"subjectAltName={named}\nbasicConstraints=CA:FALSE\n"
        (self.directory / f"{made}.ext").write_text(extensions + "extendedKeyUsage=serverAuth\n")
        self._run(
            *("req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes"),
            *("-keyout", f"{made}.key", "-out", f"{made}.csr", "-subj", "/CN=a server of a test"),
        )
        self._run(
            *("x509", "-req", "-in", f"{made}.csr", "-CA", "authority.pem"),
            *("-CAkey", "authority.key", "-CAcreateserial", "-days", "2"),
            *("-out", f"{made}.pem", "-extfile", f"{made}.ext"),
        )
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(self.directory / f"{made}.pem", self.directory / f"{made}.key")
        return context

    def filed(self) -> Path:
        """A directory that holds the authority's certificate under the name
        OpenSSL looks an authority up by, the hash of its subject: what
        ``SSL_CERT_DIR`` names."""
        directory = self.directory / "filed"
        directory.mkdir(exist_ok=True)
        named = self._run("x509", "-hash", "-noout", "-in", "authority.pem").strip()
        shutil.copyfile(self.pem, directory / f"{named}.0")
        return directory

    def close(self) -> None:
        self._held.cleanup()
