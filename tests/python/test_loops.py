"""A provider used from several event loops and from several threads: one
provider serves them all, each call completing on the loop that made it."""

from __future__ import annotations

import asyncio
import threading
import unittest

from fake_endpoint import EVENT_STREAM, JSON, Reads, ThreadedFake, Whole
from fake_openai import completion, events, local_config
from roomkit.providers.ai import AIContext, AIMessage
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])


class LoopsAndThreadsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fake = ThreadedFake()
        self.addCleanup(self.fake.stop)
        self.provider = OpenAI(OpenAIConfig(**local_config(self.fake.port)))

    async def ask(self) -> tuple[str, list[str]]:
        whole = await self.provider.generate(HELLO)
        streamed = [piece async for piece in self.provider.generate_stream(HELLO)]
        return whole.content, streamed

    def answers(self, times: int) -> None:
        replies = []
        for _ in range(times):
            replies.append(Whole(200, JSON, completion("Yes.")))
            replies.append(Reads(200, EVENT_STREAM, events("Hel", "lo.")))
        self.fake.answer(*replies)

    def test_one_provider_serves_two_event_loops_in_turn(self) -> None:
        self.answers(2)
        for _ in range(2):
            # Each run is a loop of its own, closed when it returns.
            self.assertEqual(asyncio.run(self.ask()), ("Yes.", ["Hel", "lo."]))
        self.assertEqual(len(self.fake.seen()), 4)

    def test_a_stream_begun_on_a_loop_that_closed_is_closed_with_it(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("Hel")[:1], then="silent"))

        async def begin() -> str:
            stream = self.provider.generate_stream(HELLO)
            return await anext(stream)

        # The loop closes with the stream unread: its asynchronous
        # generators are closed, and the connection with them.
        self.assertEqual(asyncio.run(begin()), "Hel")
        self.fake.closed()
        self.answers(1)
        self.assertEqual(asyncio.run(self.ask()), ("Yes.", ["Hel", "lo."]))

    def test_one_provider_serves_several_threads_at_once(self) -> None:
        threads, each = 8, 4

        async def whole() -> str:
            return (await self.provider.generate(HELLO)).content

        self.fake.answer(*(Whole(200, JSON, completion("Yes.")) for _ in range(threads * each)))
        given: list[str] = []
        failed: list[BaseException] = []
        together = threading.Barrier(threads)

        def work() -> None:
            try:
                together.wait(30)
                for _ in range(each):
                    given.append(asyncio.run(whole()))
            except BaseException as error:  # noqa: BLE001 - told to the test's thread
                failed.append(error)

        workers = [threading.Thread(target=work) for _ in range(threads)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join(60)
        self.assertEqual(failed, [])
        self.assertEqual(given, ["Yes."] * (threads * each))
        self.assertEqual(len(self.fake.seen()), threads * each)

    def test_a_provider_built_on_one_thread_is_called_from_another(self) -> None:
        self.answers(1)
        given = []
        worker = threading.Thread(target=lambda: given.append(asyncio.run(self.ask())))
        worker.start()
        worker.join(60)
        self.assertEqual(given, [("Yes.", ["Hel", "lo."])])


if __name__ == "__main__":
    unittest.main()
