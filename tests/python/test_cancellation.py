"""Cancellation and the lifetime of a request: what stops one, and that it
closes its connection. Each test waits on what the fake observed, a request
received or a connection its client closed, never on a timer."""

from __future__ import annotations

import asyncio
import gc
import unittest

from fake_endpoint import DEADLINE, EVENT_STREAM, JSON, Fake, Reads, Silent, Whole
from fake_openai import completion, events, local_config
from roomkit.providers.ai import AIContext, AIMessage, StreamTextDelta
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])


def provider_at(fake: Fake) -> OpenAI:
    return OpenAI(OpenAIConfig(**local_config(fake.port)))


def one_event_then_silence() -> Reads:
    """A stream that gives one event, then keeps its connection open."""
    return Reads(200, EVENT_STREAM, events("Hel")[:1], then="silent")


class CancellationTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        self.provider = provider_at(self.fake)

    async def test_cancelling_the_task_of_a_whole_answer_closes_its_connection(self) -> None:
        self.fake.answer(Silent())
        asking = asyncio.create_task(self.provider.generate(HELLO))
        await self.fake.received()
        asking.cancel()
        await self.fake.closed()
        with self.assertRaises(asyncio.CancelledError):
            await asking
        self.assertEqual(len(self.fake.seen), 1)

    async def test_cancelling_the_task_of_a_model_listing_closes_its_connection(self) -> None:
        self.fake.answer(Silent())
        asking = asyncio.create_task(self.provider.list_models())
        await self.fake.received()
        asking.cancel()
        await self.fake.closed()
        with self.assertRaises(asyncio.CancelledError):
            await asking

    async def test_cancelling_the_task_of_an_iteration_closes_its_connection(self) -> None:
        for stream in (self.provider.generate_structured_stream, self.provider.generate_stream):
            with self.subTest(stream=stream.__name__):
                self.fake.answer(one_event_then_silence())
                given = []
                first = asyncio.Event()

                async def read(stream=stream, given=given, first=first) -> None:
                    async for item in stream(HELLO):
                        given.append(item)
                        first.set()

                reading = asyncio.create_task(read())
                await first.wait()
                await self.fake.silent()
                reading.cancel()
                await self.fake.closed()
                with self.assertRaises(asyncio.CancelledError):
                    await reading
                self.assertEqual(len(given), 1)

    async def test_closing_a_stream_closes_its_connection(self) -> None:
        self.fake.answer(one_event_then_silence())
        stream = self.provider.generate_structured_stream(HELLO)
        self.assertEqual(await anext(stream), StreamTextDelta(text="Hel"))
        await stream.aclose()
        await self.fake.closed()
        # A stream that was closed gives nothing more, and asks nothing.
        with self.assertRaises(StopAsyncIteration):
            await anext(stream)
        self.assertEqual(len(self.fake.seen), 1)

    async def test_leaving_the_loop_of_a_stream_closes_its_connection(self) -> None:
        self.fake.answer(one_event_then_silence())
        async for event in self.provider.generate_structured_stream(HELLO):
            self.assertEqual(event.text, "Hel")
            break
        await self.fake.closed()

    async def test_a_stream_no_longer_held_closes_its_connection(self) -> None:
        self.fake.answer(one_event_then_silence())
        stream = self.provider.generate_stream(HELLO)
        self.assertEqual(await anext(stream), "Hel")
        del stream
        gc.collect()
        await self.fake.closed()

    async def test_a_stream_sends_nothing_before_its_first_item_is_asked(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("Hel", "lo.")))
        stream = self.provider.generate_structured_stream(HELLO)
        # Whatever else runs meanwhile, nothing was sent for it.
        for _ in range(100):
            await asyncio.sleep(0)
        self.assertEqual(self.fake.seen, [])
        texts = [event.text async for event in stream if isinstance(event, StreamTextDelta)]
        self.assertEqual(texts, ["Hel", "lo."])
        self.assertEqual(len(self.fake.seen), 1)

    async def test_a_stream_never_read_sends_nothing(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("Hel")))
        stream = self.provider.generate_structured_stream(HELLO)
        await stream.aclose()
        del stream
        gc.collect()
        # The next request is the first the fake receives.
        self.fake.answer(Whole(200, JSON, completion("Yes.")))
        self.assertEqual((await self.provider.generate(HELLO)).content, "Yes.")
        self.assertEqual(len(self.fake.seen), 1)
        self.assertNotIn("stream", self.fake.seen[0].body.decode())

    async def test_a_cancelled_call_leaves_the_provider_usable(self) -> None:
        self.fake.answer(Silent(), Whole(200, JSON, completion("Yes.")))
        asking = asyncio.create_task(self.provider.generate(HELLO))
        await self.fake.received()
        asking.cancel()
        await self.fake.closed()
        self.assertEqual((await self.provider.generate(HELLO)).content, "Yes.")


class EventLoopTests(unittest.IsolatedAsyncioTestCase):
    """A call waits without holding the event loop: another coroutine runs
    while it is under way."""

    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        self.provider = provider_at(self.fake)
        self.ticks = 0
        # Who waits for the ticker to reach a count.
        self.waiting: list[tuple[int, asyncio.Event]] = []
        self.ticking = asyncio.create_task(self.tick())
        self.addAsyncCleanup(self.stop_ticking)

    async def tick(self) -> None:
        while True:
            self.ticks += 1
            for until, reached in self.waiting:
                if self.ticks >= until:
                    reached.set()
            await asyncio.sleep(0)

    async def stop_ticking(self) -> None:
        self.ticking.cancel()
        await asyncio.gather(self.ticking, return_exceptions=True)

    async def ticked(self, more: int) -> None:
        """Returns once the ticker counted `more` more."""
        reached = asyncio.Event()
        self.waiting.append((self.ticks + more, reached))
        await reached.wait()

    async def test_another_coroutine_runs_during_a_whole_answer(self) -> None:
        gate = asyncio.Event()
        self.fake.answer(Whole(200, JSON, completion("Yes."), gate=gate))
        asking = asyncio.create_task(self.provider.generate(HELLO))
        await self.fake.received()
        before = self.ticks
        # The answer is held back until the ticker counted a thousand more.
        await self.ticked(1000)
        self.assertFalse(asking.done())
        gate.set()
        self.assertEqual((await asking).content, "Yes.")
        self.assertGreaterEqual(self.ticks - before, 1000)

    async def test_another_coroutine_runs_between_two_reads_of_a_stream(self) -> None:
        # Three reads, an event each: two pieces of text, then the end. The
        # fake writes each read after the first once the test lets it.
        first, second, *end = events("Hel", "lo.")
        reads = [first, second, b"".join(end)]
        let = asyncio.Semaphore(0)
        self.fake.answer(Reads(200, EVENT_STREAM, reads, step=let.acquire))
        given: asyncio.Queue[str] = asyncio.Queue()

        async def read() -> None:
            async for event in self.provider.generate_structured_stream(HELLO):
                given.put_nowait(event.type)

        reading = asyncio.create_task(read())
        for _ in range(2):
            self.assertEqual(await asyncio.wait_for(given.get(), DEADLINE), "text_delta")
            # The stream now waits for a read the fake holds back: the
            # ticker counts a thousand more meanwhile, and nothing comes.
            before = self.ticks
            await self.ticked(1000)
            self.assertGreaterEqual(self.ticks - before, 1000)
            self.assertTrue(given.empty())
            self.assertFalse(reading.done())
            let.release()
        self.assertEqual(await asyncio.wait_for(given.get(), DEADLINE), "done")
        await reading


if __name__ == "__main__":
    unittest.main()
