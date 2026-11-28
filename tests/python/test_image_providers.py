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
