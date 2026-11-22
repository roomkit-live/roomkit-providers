"""How a stream crosses from the native module: each crossing hands over
the items already received, and waits for no other."""

from __future__ import annotations

import asyncio
import gc
import json
import unittest

from fake_endpoint import DEADLINE, EVENT_STREAM, Fake, Reads
from fake_openai import events, local_config
from roomkit.providers.ai import AIContext, AIMessage, ProviderError
from roomkit_providers.llm.openai import OpenAI, OpenAIConfig

HELLO = AIContext(messages=[AIMessage(role="user", content="Hello")])
# The same request as the native module takes it.
HELLO_JSON = HELLO.model_dump_json(include={"messages"})
# The most items one crossing brings back.
BATCH_LIMIT = 64


class CrossingTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.fake = await Fake().start()
        self.addAsyncCleanup(self.fake.stop)
        self.provider = OpenAI(OpenAIConfig(**local_config(self.fake.port)))

    async def test_a_burst_is_handed_over_while_the_stream_is_still_open(self) -> None:
        # Two hundred events in one read, then the end, which the fake holds
        # back until the test read them all.
        words = [f"word{at} " for at in range(200)]
        *burst, stop, done = events(*words)
        let = asyncio.Semaphore(0)
        reads = [b"".join(burst), stop + done]
        self.fake.answer(Reads(200, EVENT_STREAM, reads, step=let.acquire))
        stream = self.provider._native.stream(HELLO_JSON, None)
        given: list[dict] = []
        while len(given) < len(words):
            crossed = json.loads(await asyncio.wait_for(stream.next(), DEADLINE))
            # A crossing brings at least an item, and no more than its bound.
            self.assertTrue(1 <= len(crossed) <= BATCH_LIMIT, len(crossed))
            given += crossed
        self.assertEqual([event["text"] for event in given], words)
        # Every piece was handed over before the stream went on.
        let.release()
        (end,) = json.loads(await stream.next())
        self.assertEqual((end["type"], end["finish_reason"]), ("done", "stop"))
        self.assertIsNone(await stream.next())
        self.assertIsNone(await stream.next())

    async def test_the_text_of_a_burst_comes_in_order_and_whole(self) -> None:
        words = [f"word{at} " for at in range(500)]
        self.fake.answer(Reads(200, EVENT_STREAM, [b"".join(events(*words))]))
        pieces = [piece async for piece in self.provider.generate_stream(HELLO)]
        self.assertEqual(pieces, words)

    async def test_closing_a_stream_ends_the_crossing_under_way(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("Hel")[:1], then="silent"))
        stream = self.provider._native.stream(HELLO_JSON, None)
        self.assertEqual(len(json.loads(await stream.next())), 1)
        waiting = asyncio.ensure_future(stream.next())
        await self.fake.silent()
        stream.close()
        self.assertIsNone(await asyncio.wait_for(waiting, DEADLINE))
        await self.fake.closed()
        self.assertIsNone(await stream.next())

    async def test_a_native_stream_no_longer_held_stops_its_reading(self) -> None:
        self.fake.answer(Reads(200, EVENT_STREAM, events("Hel")[:1], then="silent"))
        stream = self.provider._native.stream(HELLO_JSON, None)
        self.assertEqual(len(json.loads(await stream.next())), 1)
        await self.fake.silent()
        # Never closed: dropping it is what closes its connection.
        del stream
        gc.collect()
        await self.fake.closed()

    async def test_a_crossing_hands_over_everything_read_ahead(self) -> None:
        # Two hundred events, one a read. The test takes the first crossing,
        # then none until the stream read ahead as far as it may: its bound,
        # where it stays however much the fake wrote.
        words = [f"word{at} " for at in range(200)]
        *reads, stop, done = events(*words)
        self.fake.answer(Reads(200, EVENT_STREAM, [*reads, stop + done]))
        stream = self.provider._native.stream(HELLO_JSON, None)
        given = json.loads(await stream.next())
        await self.fake.served()
        # Nothing tells when the bridge filled its inbox: it is asked until
        # it did, within the deadline.
        async with asyncio.timeout(DEADLINE):
            while stream._ahead() != BATCH_LIMIT:  # noqa: ASYNC110
                await asyncio.sleep(0)
        for _ in range(100):
            await asyncio.sleep(0)
            self.assertEqual(stream._ahead(), BATCH_LIMIT)
        # One crossing brings them all, not one each.
        crossed = json.loads(await stream.next())
        self.assertEqual(len(crossed), BATCH_LIMIT)
        given += crossed
        while (crossed := await stream.next()) is not None:
            given += json.loads(crossed)
        texts = [event["text"] for event in given if event["type"] == "text_delta"]
        self.assertEqual(texts, words)

    async def test_a_stream_taken_slowly_loses_nothing(self) -> None:
        # A thousand events, one a read: the fake writes them all, and the
        # test takes none after the first crossing until the fake is done.
        words = [f"word{at} " for at in range(1000)]
        *reads, stop, done = events(*words)
        self.fake.answer(Reads(200, EVENT_STREAM, [*reads, stop + done]))
        stream = self.provider._native.stream(HELLO_JSON, None)
        given = json.loads(await stream.next())
        await self.fake.served()
        # Whatever else runs meanwhile, the next crossings hand over every
        # event in order, never more than the bound at once.
        for _ in range(100):
            await asyncio.sleep(0)
        while (crossed := await stream.next()) is not None:
            crossed = json.loads(crossed)
            self.assertLessEqual(len(crossed), BATCH_LIMIT)
            given += crossed
        texts = [event["text"] for event in given if event["type"] == "text_delta"]
        self.assertEqual(texts, words)
        self.assertEqual(given[-1]["type"], "done")

    async def test_a_failure_crosses_after_the_items_read_before_it(self) -> None:
        # Two events, then a line that is no event of the stream.
        first, second, *_ = events("Hel", "lo.")
        bad = b"data: {not json}\n\n"
        self.fake.answer(Reads(200, EVENT_STREAM, [first + second + bad]))
        given = []
        with self.assertRaises(Exception) as failed:
            async for event in self.provider.generate_structured_stream(HELLO):
                given.append(event.text)
        self.assertEqual(given, ["Hel", "lo."])
        self.assertIs(type(failed.exception), ProviderError)
        self.assertFalse(failed.exception.retryable)


if __name__ == "__main__":
    unittest.main()
