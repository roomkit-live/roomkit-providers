"""What every image provider shares, tested with none: the crossing of a
generation (its attempts told, the host's answers, its outcome) on the
native module's scripted image provider, which speaks no wire and belongs
to no vendor.

The scripted provider is in the wheel built for the tests only (the
``testing`` feature): without it these tests are skipped.
"""

from __future__ import annotations

import asyncio
import json
import subprocess
import sys
import textwrap
import unittest
from typing import Any
from unittest import mock

from roomkit.providers.ai import ProviderError
from roomkit.providers.image import ImageAttempt, ImageGenerationError
from roomkit_providers import _native
from roomkit_providers.image import _provider
from roomkit_providers.image._provider import Provider

PNG = "iVBORw0KGgo="
IMAGE = {"data": f"data:image/png;base64,{PNG}", "mime_type": "image/png"}

if not hasattr(_native, "_scripted"):
    raise unittest.SkipTest("the scripted provider is in the wheel built for the tests only")


def scripted(**script: Any) -> Provider:
    return Provider._over(_native.image_provider("scripted", json.dumps(script), None))


def asked(**given: Any) -> str:
    request = {
        "call": "generate_with_options",
        "prompt": "a fox",
        "size": None,
        "n": 1,
        "reference_images": [],
        "options": None,
        "mask": None,
        "told": True,
        **given,
    }
    return json.dumps(request)


class CrossingTests(unittest.IsolatedAsyncioTestCase):
    # Each attempt told reaches the host's callback, which the provider
    # waits for; the images then come back as RoomKit's.
    async def test_the_attempts_told_then_the_images(self) -> None:
        told: list[str] = []

        async def recorded(attempt: ImageAttempt) -> None:
            told.append(attempt.status)

        provider = scripted(tells=["started", "succeeded"], images=[IMAGE])
        images = await provider.generate_with_options("a fox", on_progress=recorded)
        self.assertEqual(told, ["started", "succeeded"])
        self.assertEqual([image.data for image in images], [IMAGE["data"]])
        self.assertEqual(provider.name, "ScriptedImageProvider")

    # A callback that raises fails the generation, raised from its failure,
    # in RoomKit's words.
    async def test_a_failed_callback_fails_the_generation(self) -> None:
        failure = RuntimeError("storage unavailable")

        async def failing(attempt: ImageAttempt) -> None:
            raise failure

        provider = scripted(tells=["started"], images=[IMAGE])
        with self.assertRaises(ImageGenerationError) as raised:
            await provider.generate_with_options("a fox", on_progress=failing)
        self.assertIs(raised.exception.__cause__, failure)
        said = "Image progress callback failed: storage unavailable"
        self.assertEqual(raised.exception.args[0], said)

    # An item stays the first a call gives until it is taken: one a
    # cancelled crossing carried is given again.
    async def test_an_item_not_taken_is_given_again(self) -> None:
        call = _native.image_provider("scripted", json.dumps({"tells": ["started"]}), None)
        generation = call.generation(asked())
        async with asyncio.timeout(5):
            first = json.loads(await generation.next())
            self.assertEqual(json.loads(await generation.next()), first)
            generation.taken(first["token"])
            generation.reply(first["token"], None)
            outcome = json.loads(await generation.next())
            self.assertEqual(outcome["outcome"], {"images": []})
            generation.taken(outcome["token"])
            self.assertIsNone(await generation.next())

    # An attempt told that does not read fails as a callback that raised:
    # the provider, which waits for an answer, is answered.
    async def test_an_attempt_that_does_not_read_fails_the_generation(self) -> None:
        read: list[Any] = []

        class Unread:
            """Reads every attempt but the first: the one told."""

            @staticmethod
            def model_validate(written: Any) -> ImageAttempt:
                read.append(written)
                if len(read) == 1:
                    raise ValueError("no attempt")
                return ImageAttempt.model_validate(written)

        async def recorded(attempt: ImageAttempt) -> None:
            raise AssertionError("an unread attempt is told")

        provider = scripted(tells=["started"], images=[IMAGE])
        with (
            mock.patch.object(_provider, "ImageAttempt", Unread),
            self.assertRaises(ImageGenerationError) as raised,
        ):
            async with asyncio.timeout(5):
                await provider.generate_with_options("a fox", on_progress=recorded)
        self.assertIn("no attempt", raised.exception.args[0])

    # A callback whose failure has no words still fails the generation: the
    # provider, which waits for an answer, has one.
    async def test_a_callback_failing_without_words_fails_the_generation(self) -> None:
        class Unwritten(Exception):
            def __str__(self) -> str:
                raise RuntimeError("no words")

        async def failing(attempt: ImageAttempt) -> None:
            raise Unwritten

        provider = scripted(tells=["started"], images=[IMAGE])
        with self.assertRaises(ImageGenerationError):
            async with asyncio.timeout(5):
                await provider.generate_with_options("a fox", on_progress=failing)

    # A panic of the generation is the provider's failure.
    async def test_a_panic_is_the_provider_s_failure(self) -> None:
        with self.assertRaises(ProviderError) as raised:
            await scripted(panics=True).generate("a fox")
        self.assertNotIsInstance(raised.exception, ImageGenerationError)
        self.assertEqual(raised.exception.args[0], "the provider stopped on an internal error")

    # A provider that does not name itself as its vendor states is not
    # built.
    def test_a_misnamed_provider_is_refused(self) -> None:
        with self.assertRaises(Exception) as raised:
            _native.image_provider("scripted", json.dumps({"name": "AnotherProvider"}), None)
        self.assertIn("does not name itself as its vendor states", str(raised.exception))

    # A request that does not read is refused before anything is sent.
    def test_a_request_that_does_not_read_is_refused(self) -> None:
        call = _native.image_provider("scripted", "{}", None)
        with self.assertRaises(Exception) as raised:
            call.generation(json.dumps({"prompt": "a fox"}))
        self.assertIn("the request is refused", str(raised.exception))


class TellingTests(unittest.IsolatedAsyncioTestCase):
    # A telling cancelled as soon as it is made still answers its attempt:
    # the provider waits for that answer, so a generation whose tellings a
    # shutdown cancels still ends.
    async def test_a_telling_cancelled_at_once_still_answers(self) -> None:
        class Call:
            def __init__(self) -> None:
                self.answers: list[tuple[int, str | None]] = []

            def reply(self, token: int, failed: str | None) -> None:
                self.answers.append((token, failed))

        entered: list[str] = []

        async def waiting(attempt: ImageAttempt) -> None:
            entered.append(attempt.status)
            await asyncio.Event().wait()

        call = Call()
        generation = _provider._Generation(call, waiting, "scripted")  # type: ignore[arg-type]
        generation._tell({"told": {"index": 0, "status": "unknown"}, "token": 7})
        # The callback is told at once, as RoomKit's attempts tell it.
        self.assertEqual(entered, ["unknown"])
        for task in list(generation._telling):
            task.cancel()
        async with asyncio.timeout(5):
            await generation._settle(cancel=False)
        self.assertEqual([token for token, _ in call.answers], [7])


class ForkTests(unittest.TestCase):
    # A forked child answers no attempt of its parent's generation and
    # takes none of its items: their locks may be held by a parent thread.
    def test_a_forked_child_answers_nothing(self) -> None:
        script = textwrap.dedent("""
            import asyncio, json, os, warnings
            from roomkit_providers import _native

            warnings.filterwarnings("ignore", category=DeprecationWarning)
            asked = json.dumps({"call": "generate_with_options", "prompt": "a fox", "size": None,
                "n": 1, "reference_images": [], "options": None, "mask": None, "told": True})

            async def first():
                held = _native.image_provider("scripted", json.dumps({"tells": ["started"]}), None)
                generation = held.generation(asked)
                return held, generation, json.loads(await generation.next())

            held, generation, item = asyncio.run(first())
            pid = os.fork()
            if pid == 0:
                refused = 0
                for call in (lambda: generation.reply(item["token"], None),
                             lambda: generation.taken(item["token"])):
                    try:
                        call()
                    except RuntimeError:
                        refused += 1
                os._exit(0 if refused == 2 else 7)
            _, status = os.waitpid(pid, 0)
            print("child:", os.waitstatus_to_exitcode(status))
        """)
        result = subprocess.run(
            [sys.executable, "-X", "faulthandler", "-I", "-c", script],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("child: 0", result.stdout)


if __name__ == "__main__":
    unittest.main()
