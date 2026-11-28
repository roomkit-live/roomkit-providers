"""What the replay of the image cases cannot show of the package's image
providers: a generation cancelled while its requests are in flight, what a
generation failed by its callback is raised from, and how a provider is
built from either configuration."""

from __future__ import annotations

import asyncio
import json
import time
import unittest
from typing import Any

from fake_endpoint import Fake, Silent, ThreadedFake, Whole
from replay_cases import CLOCK
from roomkit.providers.gemini import GeminiImageConfig as RoomKitGeminiImageConfig
from roomkit.providers.image import ImageAttempt, ImageGenerationError, ImageModelInfo
from roomkit_providers import _native
from roomkit_providers.image.gemini import GeminiImage, GeminiImageConfig
from roomkit_providers.image.openai import OpenAIImage, OpenAIImageConfig

KEY = "placeholder-not-a-key"
PNG = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAA"
    "AABJRU5ErkJggg=="
)
CANCELLED = "Cancelled locally; the provider may still bill this request"


def interaction() -> bytes:
    image = {"type": "image", "data": PNG, "mime_type": "image/png"}
    step = {"type": "model_output", "content": [image]}
    return json.dumps({"id": "int_1", "status": "completed", "steps": [step]}).encode()


def replaying(provider: Any, vendor: str, fake: Fake) -> None:
    """Sends `provider` to `fake`."""
    built = provider._config
    key = getattr(built, type(built)._key, None)
    native, _waits = _native._image_replaying(
        vendor, built.model_dump_json(), key, "127.0.0.1", fake.port, CLOCK
    )
    provider._hold(native)


class CancellationTests(unittest.IsolatedAsyncioTestCase):
    # Cancelling the task that awaits a generation stops its requests, and
    # tells the callback each attempt then in flight unknown before the
    # cancellation goes on, as RoomKit's own providers do.
    async def test_attempts_in_flight_are_told_unknown(self) -> None:
        told: list[ImageAttempt] = []

        async def recorded(attempt: ImageAttempt) -> None:
            told.append(attempt)

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(Silent(), Silent())
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", n=2, on_progress=recorded)
            )
            async with asyncio.timeout(5):
                await fake.received()
                await fake.received()
            generating.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await generating
        moves = sorted((attempt.index, attempt.status, attempt.error) for attempt in told)
        self.assertEqual(
            moves,
            [
                (0, "started", None),
                (0, "unknown", CANCELLED),
                (1, "started", None),
                (1, "unknown", CANCELLED),
            ],
        )

    # A callback that fails fails the generation, which is raised from the
    # callback's failure, as RoomKit's: the image drawn is kept.
    async def test_a_failed_callback_is_the_cause(self) -> None:
        failure = RuntimeError("storage unavailable")

        async def failing(attempt: ImageAttempt) -> None:
            if attempt.status == "succeeded":
                raise failure

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            json_headers = {"content-type": "application/json"}
            fake.answer(Whole(200, json_headers, interaction()))
            with self.assertRaises(ImageGenerationError) as raised:
                await provider.generate_with_options("a fox", on_progress=failing)
        error = raised.exception
        self.assertIs(error.__cause__, failure)
        self.assertEqual(
            error.args[0], "gemini: Image progress callback failed: storage unavailable"
        )
        self.assertEqual([attempt.status for attempt in error.attempts], ["succeeded"])
        self.assertEqual(len(error.results), 1)

    # A generation another attempt failed first is not raised from the
    # callback that failed on a later one.
    async def test_a_callback_failing_second_is_no_cause(self) -> None:
        async def failing(attempt: ImageAttempt) -> None:
            if attempt.status == "succeeded":
                raise RuntimeError("storage unavailable")

        refused = json.dumps({"error": {"code": 429, "message": "slow", "status": "X"}})
        json_headers = {"content-type": "application/json"}
        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(
                Whole(429, json_headers, refused.encode()), Whole(200, json_headers, interaction())
            )
            with self.assertRaises(ImageGenerationError) as raised:
                await provider.generate_with_options("a fox", n=2, on_progress=failing)
        error = raised.exception
        self.assertEqual(error.status_code, 429)
        self.assertNotIsInstance(error.__cause__, RuntimeError)


class ConcurrencyTests(unittest.IsolatedAsyncioTestCase):
    # The callbacks of attempts made at once run at once, as RoomKit's
    # provider runs them: a callback that waits for another attempt's is
    # answered.
    async def test_callbacks_of_attempts_made_at_once_run_at_once(self) -> None:
        barrier = asyncio.Barrier(2)

        async def waiting(attempt: ImageAttempt) -> None:
            if attempt.status == "started":
                await barrier.wait()

        json_headers = {"content-type": "application/json"}
        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(*(Whole(200, json_headers, interaction()) for _ in range(2)))
            async with asyncio.timeout(5):
                images = await provider.generate_with_options("a fox", n=2, on_progress=waiting)
        self.assertEqual(len(images), 2)

    # A cancellation that finds an attempt's telling on its way to Python
    # still tells it: the item stays the call's first until it is taken.
    async def test_a_telling_a_cancellation_meets_is_told(self) -> None:
        told: list[tuple[int, str]] = []
        started = asyncio.Event()

        async def recorded(attempt: ImageAttempt) -> None:
            told.append((attempt.index, attempt.status))
            if attempt.status == "started":
                started.set()

        fake = ThreadedFake()
        try:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(Whole(200, {"content-type": "application/json"}, interaction()))
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", on_progress=recorded)
            )
            await started.wait()
            # The loop busy while the fake, on its own thread, answers: the
            # provider tells the attempt succeeded meanwhile.
            time.sleep(0.5)  # noqa: ASYNC251
            generating.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await generating
        finally:
            fake.stop()
        self.assertEqual(told[0], (0, "started"))
        self.assertEqual(len(told), 2, told)
        self.assertIn(told[1], [(0, "succeeded"), (0, "unknown")])

    # Cancelled while callbacks run, a generation cancels them, tells no
    # attempt that only started, and leaves no callback running.
    async def test_callbacks_under_way_are_cancelled_with_it(self) -> None:
        cancelled: list[int] = []
        entered: list[int] = []
        both = asyncio.Event()

        async def hanging(attempt: ImageAttempt) -> None:
            entered.append(attempt.index)
            if len(entered) == 2:
                both.set()
            try:
                await asyncio.Event().wait()
            except asyncio.CancelledError:
                cancelled.append(attempt.index)
                raise

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", n=2, on_progress=hanging)
            )
            async with asyncio.timeout(5):
                await both.wait()
            generating.cancel()
            # Waited for, never cancelled again: the cancellation alone
            # must end it.
            await asyncio.wait({generating}, timeout=2)
            self.assertTrue(generating.cancelled(), "the cancelled generation did not end")
            self.assertEqual(fake.seen, [])
        self.assertEqual(sorted(cancelled), [0, 1])
        self.assertEqual(_tellings(), [])

    # Cancelled again while it tells the attempts it left in flight, a
    # generation cancels those callbacks too and waits for them to end:
    # none is left running, its clean-up done.
    async def test_a_second_cancellation_leaves_no_callback_running(self) -> None:
        unknown = asyncio.Event()
        cleaned: list[str] = []

        async def slow(attempt: ImageAttempt) -> None:
            if attempt.status != "unknown":
                return
            unknown.set()
            try:
                await asyncio.sleep(5)
            except asyncio.CancelledError:
                await asyncio.sleep(0.2)
                cleaned.append(attempt.status)
                raise

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(Silent())
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", on_progress=slow)
            )
            async with asyncio.timeout(5):
                await fake.received()
                generating.cancel()
                await unknown.wait()
            generating.cancel()
            await asyncio.wait({generating}, timeout=2)
            self.assertTrue(generating.cancelled(), "the cancelled generation did not end")
            self.assertEqual(cleaned, ["unknown"])
            self.assertEqual(_tellings(), [])

    # Cancelled again while the callbacks it cancelled clean up, a generation
    # cancels them again, as RoomKit's cancels its attempts again, waits for
    # them to end, and still tells the attempt it left in flight.
    async def test_a_cancellation_during_the_clean_up_waits_for_it(self) -> None:
        entered, cleaning = asyncio.Event(), asyncio.Event()
        ended: list[int] = []
        told: list[tuple[int, str]] = []

        async def second_hangs(attempt: ImageAttempt) -> None:
            told.append((attempt.index, attempt.status))
            if (attempt.index, attempt.status) != (1, "started"):
                return
            entered.set()
            try:
                await asyncio.Event().wait()
            except asyncio.CancelledError:
                cleaning.set()
                await asyncio.sleep(0.2)
                raise
            finally:
                ended.append(attempt.index)

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(Silent())
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", n=2, on_progress=second_hangs)
            )
            async with asyncio.timeout(5):
                await fake.received()
                await entered.wait()
                generating.cancel()
                await cleaning.wait()
            generating.cancel()
            await asyncio.wait({generating}, timeout=2)
            self.assertTrue(generating.cancelled(), "the cancelled generation did not end")
            self.assertEqual(ended, [1])
            self.assertIn((0, "unknown"), told)
            self.assertEqual(_tellings(), [])

    # Cancelled again while it waits for what to tell, a generation still
    # tells each attempt it left in flight unknown.
    async def test_a_second_cancellation_still_tells_the_attempts_in_flight(self) -> None:
        told: list[tuple[int, str]] = []

        async def recorded(attempt: ImageAttempt) -> None:
            told.append((attempt.index, attempt.status))

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(Silent(), Silent())
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", n=2, on_progress=recorded)
            )
            async with asyncio.timeout(5):
                await fake.received()
                await fake.received()
            generating.cancel()
            await asyncio.sleep(0)
            generating.cancel()
            await asyncio.wait({generating}, timeout=2)
            self.assertTrue(generating.cancelled(), "the cancelled generation did not end")
        unknown = sorted(index for index, status in told if status == "unknown")
        self.assertEqual(unknown, [0, 1])

    # A callback that fails while the cancelled generation tells it the
    # attempt left in flight is noted on the cancellation, in RoomKit's words.
    async def test_a_failure_to_report_is_noted(self) -> None:
        for failure, said in [
            ("store down", "gemini: Image progress callback failed: store down"),
            # A failure that names the provider is not led by its name.
            ("Gemini store down", "Image progress callback failed: Gemini store down"),
        ]:
            with self.subTest(failure=failure):
                notes = await self._noted(failure)
                self.assertEqual(notes, [f"Failed to report unknown image outcome: {said}"])

    async def _noted(self, failure: str) -> list[str]:
        async def failing(attempt: ImageAttempt) -> None:
            if attempt.status == "unknown":
                raise RuntimeError(failure)

        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(Silent())
            generating = asyncio.create_task(
                provider.generate_with_options("a fox", on_progress=failing)
            )
            async with asyncio.timeout(5):
                await fake.received()
                generating.cancel()
                with self.assertRaises(asyncio.CancelledError) as raised:
                    await generating
        return raised.exception.__notes__

    # Two callbacks that fail: the generation is raised from the failure of
    # the first attempt's, as RoomKit's.
    async def test_the_first_attempt_s_callback_is_the_cause(self) -> None:
        failures = {0: RuntimeError("store 0 down"), 1: RuntimeError("store 1 down")}

        async def failing(attempt: ImageAttempt) -> None:
            if attempt.status == "succeeded":
                # The first attempt's fails last: the cause is the first by
                # its index, not by its time.
                if attempt.index == 0:
                    await asyncio.sleep(0.1)
                raise failures[attempt.index]

        json_headers = {"content-type": "application/json"}
        async with Fake() as fake:
            provider = GeminiImage(GeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
            replaying(provider, "gemini", fake)
            fake.answer(*(Whole(200, json_headers, interaction()) for _ in range(2)))
            with self.assertRaises(ImageGenerationError) as raised:
                await provider.generate_with_options("a fox", n=2, on_progress=failing)
        self.assertIs(raised.exception.__cause__, failures[0])


def _tellings() -> list[asyncio.Task[Any]]:
    """The tasks that tell a host's callback an attempt, still running."""
    return [
        task
        for task in asyncio.all_tasks()
        if not task.done() and "_told" in getattr(task.get_coro(), "__qualname__", "")
    ]


class ConfigurationTests(unittest.TestCase):
    # A provider is built from the package's configuration or RoomKit's,
    # and answers RoomKit's names and catalogue.
    def test_either_configuration_builds_the_provider(self) -> None:
        ours = GeminiImage(GeminiImageConfig(api_key=KEY))
        theirs = GeminiImage(RoomKitGeminiImageConfig(api_key=KEY, model="gemini-3-pro-image"))
        self.assertEqual(
            (ours.name, ours.model_name), ("GeminiImageProvider", "gemini-nano-banana-2.1")
        )
        self.assertEqual(theirs.model_name, "gemini-3-pro-image")
        self.assertTrue(ours.supports_editing)
        self.assertIsInstance(ours.catalog_entry(), ImageModelInfo)
        self.assertTrue(
            all(isinstance(entry, ImageModelInfo) for entry in GeminiImage.available_models())
        )
        openai = OpenAIImage(OpenAIImageConfig(api_key=KEY, model="gpt-image-2"))
        self.assertEqual(
            (openai.name, openai.catalog_entry().id), ("OpenAIImageProvider", "gpt-image-2")
        )

    # A configuration never shows its key, and another provider's is refused.
    def test_a_configuration_keeps_its_key(self) -> None:
        self.assertNotIn(KEY, repr(GeminiImageConfig(api_key=KEY)))
        with self.assertRaises(TypeError):
            GeminiImage(OpenAIImageConfig(api_key=KEY, model="gpt-image-2"))  # type: ignore[arg-type]


if __name__ == "__main__":
    unittest.main()
