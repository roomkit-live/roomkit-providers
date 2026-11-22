"""Interpreter exit with a call still running, and a forked child: neither
crashes nor hangs, and a call the exit stops closes its connection."""

from __future__ import annotations

import os
import subprocess
import sys
import textwrap
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent

# What each child starts with: the fake of the endpoint on a thread of its
# own, which the child's event loops and its exit do not stop.
PREAMBLE = """
import asyncio
import json
import sys

sys.path.insert(0, sys.argv[1])
from fake_endpoint import EVENT_STREAM, JSON, Reads, Silent, ThreadedFake, Whole
from fake_openai import completion, events, local_config

from roomkit.providers.ai import AIContext, AIMessage

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
# The same request as the native module takes it.
HELLO_JSON = HELLO.model_dump_json(include={"messages"})


def provider_at(fake):
    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

    return OpenAI(OpenAIConfig(**local_config(fake.port)))


def answers(fake, times=1):
    fake.answer(*(Whole(200, JSON, completion("Yes.")) for _ in range(times)))


async def ask(provider):
    return (await provider.generate(HELLO)).content
"""


class ChildProcess(unittest.TestCase):
    """A test whose scenario runs in an interpreter of its own."""

    def run_child(self, body: str, *, timeout: float = 60) -> subprocess.CompletedProcess[str]:
        script = PREAMBLE + textwrap.dedent(body)
        # An explicit list of arguments also covers paths containing spaces.
        result = subprocess.run(
            [sys.executable, "-X", "faulthandler", "-I", "-c", script, str(HERE)],
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("Fatal Python error", result.stderr)
        self.assertNotIn("Exception ignored", result.stderr)
        self.assertNotIn("Event loop is closed", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        return result


class InterpreterShutdownTests(ChildProcess):
    def test_calls_still_running_at_exit_are_stopped_and_their_connections_closed(self) -> None:
        result = self.run_child("""
            import atexit

            fake = ThreadedFake()

            def observe():
                # The exit hook of the native module ran first: it stopped
                # the call and the stream, whose connections the fake then
                # saw closed.
                fake.closed()
                fake.closed()
                print("the exit closed the connections")

            # Registered before the package is imported, so it runs after
            # the package's own exit hook.
            atexit.register(observe)
            provider = provider_at(fake)
            from roomkit_providers import _native

            async def begin():
                asking = provider._native.generate(HELLO_JSON, None)
                await asyncio.to_thread(fake.received)
                # A stream that gave an item and is read no further.
                stream = provider._native.stream(HELLO_JSON, None)
                assert len(json.loads(await stream.next())) == 1
                return asking, stream

            first = events("Hel")[:1]
            fake.answer(Silent(), Reads(200, EVENT_STREAM, first, then="silent"))
            loop = asyncio.new_event_loop()
            pending, stream = loop.run_until_complete(begin())
            loop.close()
            assert _native._activity() > 0
            print("exiting with a call and a stream under way")
        """)
        self.assertIn("exiting with a call and a stream under way", result.stdout)
        self.assertIn("the exit closed the connections", result.stdout)

    def test_cancelled_calls_are_drained_before_the_interpreter_exits(self) -> None:
        for attempt in range(3):
            with self.subTest(attempt=attempt):
                self.run_child("""
                    fake = ThreadedFake()
                    provider = provider_at(fake)
                    fake.answer(*(Silent() for _ in range(32)))

                    async def main():
                        pending = [provider._native.generate(HELLO_JSON, None) for _ in range(32)]
                        for future in pending:
                            future.cancel()
                        return pending

                    sys.setswitchinterval(0.1)
                    pending = asyncio.run(main())
                """)

    def test_shutdown_waits_for_the_calls_and_refuses_new_ones(self) -> None:
        result = self.run_child("""
            fake = ThreadedFake()
            provider = provider_at(fake)
            from roomkit_providers import _native

            async def main():
                fake.answer(*(Silent() for _ in range(8)))
                pending = [provider._native.generate(HELLO_JSON, None) for _ in range(8)]
                drained = _native._shutdown()
                assert drained > 0, drained
                assert _native._activity() == 0
                assert _native._shutdown() == 0
                try:
                    await ask(provider)
                except RuntimeError as error:
                    assert "shutting down" in str(error), error
                else:
                    raise AssertionError("shutdown reopened the bridge")
                try:
                    async for _ in provider.generate_stream(HELLO):
                        pass
                except RuntimeError as error:
                    assert "shutting down" in str(error), error
                else:
                    raise AssertionError("shutdown reopened the bridge")
                print("drained and permanently closed")

            asyncio.run(main())
        """)
        self.assertIn("drained and permanently closed", result.stdout)

    def test_the_exit_hook_releases_the_interpreter_while_it_waits(self) -> None:
        result = self.run_child("""
            from roomkit_providers import _native

            fake = ThreadedFake()
            provider = provider_at(fake)
            fake.answer(*(Silent() for _ in range(4)))

            async def main():
                pending = [provider._native.generate(HELLO_JSON, None) for _ in range(4)]
                for _ in pending:
                    fake.received()
                # The hook returns once each stopped call's end was handed
                # to Python, by a thread that takes the interpreter to do
                # it: it could not, had the hook kept the interpreter.
                stopped = _native._shutdown()
                assert stopped >= 4, stopped
                assert _native._activity() == 0
                # What was handed over: each call failed of the exit.
                for call in pending:
                    try:
                        await call
                    except RuntimeError as error:
                        assert "shutting down" in str(error), error
                    else:
                        raise AssertionError("a call was answered")
                print("the hook waited without the interpreter")

            asyncio.run(main())
        """)
        self.assertIn("the hook waited without the interpreter", result.stdout)

    def test_closing_a_provider_once_the_bridge_is_down_does_nothing(self) -> None:
        result = self.run_child("""
            import atexit

            def closes_late():
                # Registered before the package is imported: it runs after
                # the package's own exit hook.
                assert asyncio.run(provider.close()) is None
                print("closed after the exit hook")

            atexit.register(closes_late)
            fake = ThreadedFake()
            provider = provider_at(fake)
            answers(fake)
            assert asyncio.run(ask(provider)) == "Yes."
        """)
        self.assertIn("closed after the exit hook", result.stdout)

    def test_a_completion_after_its_loop_closed_is_silent(self) -> None:
        # A host abandons a call, closes its loop and keeps running: the
        # answer that comes afterwards is dropped, and nothing is printed.
        result = self.run_child("""
            fake = ThreadedFake()
            provider = provider_at(fake)
            from roomkit_providers import _native

            async def begin(gate):
                fake.answer(Whole(200, JSON, completion("Yes."), gate=gate))
                asking = provider._native.generate(HELLO_JSON, None)
                await asyncio.to_thread(fake.received)
                return asking

            async def make_gate():
                return asyncio.Event()

            async def open_gate(gate):
                gate.set()

            gate = fake.run(make_gate())
            loop = asyncio.new_event_loop()
            pending = loop.run_until_complete(begin(gate))
            loop.close()
            fake.run(open_gate(gate))
            fake.run(fake.fake.served())
            import time
            deadline = time.monotonic() + 30
            while _native._activity() and time.monotonic() < deadline:
                time.sleep(0.01)
            assert _native._activity() == 0, _native._activity()
            assert not pending.done()
            print("completed after the loop closed")
        """)
        self.assertIn("completed after the loop closed", result.stdout)

    def test_importing_starts_no_native_thread_and_shutdown_needs_none(self) -> None:
        if sys.platform != "linux":
            self.skipTest("observe Linux threads")
        result = subprocess.run(
            [
                sys.executable,
                "-I",
                "-c",
                textwrap.dedent("""
                    import os
                    import roomkit_providers
                    from roomkit_providers import _native
                    from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

                    config = OpenAIConfig(api_key="placeholder-not-a-key", model="gpt-4o")
                    provider = OpenAI(config)
                    assert provider.supports_vision
                    assert len(os.listdir("/proc/self/task")) == 1
                    assert _native._shutdown() == 0
                    assert _native._shutdown() == 0
                    assert len(os.listdir("/proc/self/task")) == 1
                    print("no runtime needed")
                """),
            ],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("no runtime needed", result.stdout)


@unittest.skipUnless(hasattr(os, "fork"), "os.fork")
class ForkTests(ChildProcess):
    """A child forked after its parent's first native call cannot make one:
    the runtime's threads are not copied. It is refused at once."""

    def test_a_child_forked_after_a_native_call_is_refused_at_once(self) -> None:
        result = self.run_child("""
            import multiprocessing
            import time
            import warnings

            # Forking a process with threads is the scenario under test.
            warnings.filterwarnings("ignore", category=DeprecationWarning)
            fake = ThreadedFake()
            provider = provider_at(fake)

            def child(queue):
                started = time.monotonic()
                outcomes = []
                for call in (ask(provider), provider.list_models()):
                    try:
                        asyncio.run(asyncio.wait_for(call, 5))
                    except BaseException as error:
                        outcomes.append((type(error).__name__, str(error)))
                    else:
                        outcomes.append(("answered", ""))
                try:
                    provider._native.stream(HELLO_JSON, None)
                except BaseException as error:
                    outcomes.append((type(error).__name__, str(error)))
                queue.put((outcomes, time.monotonic() - started))

            answers(fake)
            assert asyncio.run(ask(provider)) == "Yes."
            context = multiprocessing.get_context("fork")
            queue = context.Queue()
            process = context.Process(target=child, args=(queue,))
            process.start()
            outcomes, elapsed = queue.get(timeout=30)
            process.join(30)
            for name, message in outcomes:
                print(f"{name}: {message}")
            assert [name for name, _ in outcomes] == ["RuntimeError"] * 3, outcomes
            assert process.exitcode == 0, process.exitcode
            # The parent keeps its runtime.
            answers(fake)
            assert asyncio.run(ask(provider)) == "Yes."
            print("the parent still answers")
        """)
        self.assertIn("RuntimeError: a provider cannot be used in a process forked", result.stdout)
        self.assertIn("spawn", result.stdout)
        self.assertIn("the parent still answers", result.stdout)

    def test_a_forked_child_is_refused_without_entering_the_native_module(self) -> None:
        result = self.run_child("""
            import multiprocessing
            import warnings

            import traceback

            import roomkit_providers._process
            import roomkit_providers.llm._config
            import roomkit_providers.llm._provider
            from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

            warnings.filterwarnings("ignore", category=DeprecationWarning)
            fake = ThreadedFake()
            provider = provider_at(fake)

            class Entered:
                # The native module, to a child that must not enter it.
                def __getattr__(self, name):
                    raise AssertionError(f"the child entered the native module: {name}")

            async def reads(stream):
                return [event async for event in stream]

            def child(queue):
                for module in (
                    roomkit_providers._process,
                    roomkit_providers.llm._config,
                    roomkit_providers.llm._provider,
                ):
                    module._native = Entered()
                provider._native = Entered()
                refused, raised_in = [], []
                for attempt in (
                    lambda: asyncio.run(ask(provider)),
                    lambda: asyncio.run(provider.list_models()),
                    lambda: asyncio.run(reads(provider.generate_structured_stream(HELLO))),
                    lambda: asyncio.run(reads(provider.generate_stream(HELLO))),
                    lambda: OpenAI(config),
                    # A configuration with a switch its model decides, and
                    # one that states them all.
                    lambda: OpenAIConfig(api_key="placeholder-not-a-key", model="gpt-5.1"),
                    lambda: OpenAIConfig(**local_config(fake.port), **STATED),
                ):
                    try:
                        attempt()
                    except RuntimeError as error:
                        refused.append("forked" in str(error))
                        frames = traceback.extract_tb(error.__traceback__)
                        raised_in += [frame.name for frame in frames]
                    except BaseException as error:
                        refused.append(repr(error))
                    else:
                        refused.append("not refused")
                # The refusal comes before anything is read: none is raised
                # while a configuration is checked.
                assert "model_post_init" not in raised_in, raised_in
                # What a provider says of itself was read when it was built,
                # and its catalogue when its class was defined; closing it
                # does nothing, without failing.
                said = (provider.name, provider.model_name, provider.supports_vision)
                listed = len(OpenAI.available_models())
                closed = asyncio.run(provider.close())
                queue.put((refused, said, listed, closed))

            STATED = {"use_max_completion_tokens": False, "supports_custom_temperature": True}
            config = OpenAIConfig(**local_config(fake.port))
            answers(fake)
            assert asyncio.run(ask(provider)) == "Yes."
            context = multiprocessing.get_context("fork")
            queue = context.Queue()
            process = context.Process(target=child, args=(queue,))
            process.start()
            refused, said, listed, closed = queue.get(timeout=30)
            process.join(30)
            assert refused == [True] * 7, refused
            assert listed == len(OpenAI.available_models()) > 0, listed
            assert said == ("OpenAIAIProvider", "local-model", provider.supports_vision), said
            assert closed is None
            assert process.exitcode == 0, process.exitcode
            print("refused in Python")
        """)
        self.assertIn("refused in Python", result.stdout)

    def test_a_forked_child_s_exit_does_not_enter_the_native_module(self) -> None:
        result = self.run_child("""
            import os
            import warnings

            import roomkit_providers._process

            # Forking a process with threads is the scenario under test.
            warnings.filterwarnings("ignore", category=DeprecationWarning)
            fake = ThreadedFake()
            provider = provider_at(fake)

            class Entered:
                # The native module, to a child that must not enter it: the
                # child ends there, and its parent reads how it ended.
                def __getattr__(self, name):
                    os._exit(3)

            answers(fake)
            assert asyncio.run(ask(provider)) == "Yes."
            pid = os.fork()
            if pid == 0:
                roomkit_providers._process._native = Entered()
                # The interpreter's own exit, which runs the package's hook.
                sys.exit(0)
            _, status = os.waitpid(pid, 0)
            assert os.waitstatus_to_exitcode(status) == 0, os.waitstatus_to_exitcode(status)
            # The hook is what the child's exit ran: the parent's enters.
            entered, native = [], roomkit_providers._process._native
            roomkit_providers._process._native = type(
                "Counting", (), {"_shutdown": lambda self: entered.append("shutdown")}
            )()
            roomkit_providers._process._leave()
            roomkit_providers._process._native = native
            assert entered == ["shutdown"], entered
            print("the child's exit entered nothing")
        """)
        self.assertIn("the child's exit entered nothing", result.stdout)

    def test_a_child_forked_during_a_call_exits_without_waiting_for_it(self) -> None:
        result = self.run_child("""
            import os
            import signal
            import time
            import warnings

            # Forking a process with threads is the scenario under test.
            warnings.filterwarnings("ignore", category=DeprecationWarning)
            fake = ThreadedFake()
            provider = provider_at(fake)
            from roomkit_providers import _native

            async def begin():
                asking = provider._native.generate(HELLO_JSON, None)
                await asyncio.to_thread(fake.received)
                return asking

            fake.answer(Silent())
            loop = asyncio.new_event_loop()
            pending = loop.run_until_complete(begin())
            assert _native._activity() > 0
            pid = os.fork()
            if pid == 0:
                try:
                    asyncio.run(asyncio.wait_for(ask(provider), 5))
                except RuntimeError as error:
                    print("child refused:", type(error).__name__, flush=True)
                # A normal exit runs the inherited native exit hook, which
                # waits for none of the parent's calls.
                sys.exit(0)
            deadline = time.monotonic() + 30
            while (status := os.waitpid(pid, os.WNOHANG))[0] == 0:
                if time.monotonic() > deadline:
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
                    raise SystemExit("the forked child hung at exit")
                time.sleep(0.01)
            assert os.waitstatus_to_exitcode(status[1]) == 0, status
            pending.cancel()
            loop.run_until_complete(asyncio.sleep(0))
            loop.close()
            print("forked child exited")
        """)
        self.assertIn("child refused: RuntimeError", result.stdout)
        self.assertIn("forked child exited", result.stdout)

    def test_a_child_forked_before_any_native_call_can_call(self) -> None:
        result = self.run_child("""
            import multiprocessing
            import warnings

            # Forking a process with threads is the scenario under test.
            warnings.filterwarnings("ignore", category=DeprecationWarning)
            fake = ThreadedFake()
            # Importing the package and building a provider make no native
            # call.
            provider = provider_at(fake)

            def child(queue):
                queue.put(asyncio.run(asyncio.wait_for(ask(provider), 10)))

            answers(fake, 2)
            context = multiprocessing.get_context("fork")
            queue = context.Queue()
            process = context.Process(target=child, args=(queue,))
            process.start()
            print("child read:", queue.get(timeout=30))
            process.join(30)
            assert process.exitcode == 0, process.exitcode
            print("parent read:", asyncio.run(ask(provider)))
        """)
        self.assertIn("child read: Yes.", result.stdout)
        self.assertIn("parent read: Yes.", result.stdout)

    def test_importing_registers_no_fork_hook(self) -> None:
        # A native hook would run PyO3 code inside every child's fork().
        result = self.run_child("""
            import os

            hooks = []
            register = os.register_at_fork

            def recording(**callbacks):
                hooks.append(callbacks)
                register(**callbacks)

            os.register_at_fork = recording
            import roomkit_providers
            import roomkit_providers.llm.openai

            native = [
                callback
                for callbacks in hooks
                for callback in callbacks.values()
                if getattr(callback, "__module__", "").startswith("roomkit_providers")
            ]
            assert native == [], native
            print("no fork hook")
        """)
        self.assertIn("no fork hook", result.stdout)


if __name__ == "__main__":
    unittest.main()
