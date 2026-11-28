"""The recorded image cases replayed through the package's image providers.

Each case of ``python_image_cases.json``, captured on RoomKit's own image
provider of the vendor, is asked of the package's provider as a host asks
it, against a local fake of the endpoint: the requests sent, the images or
the failure given and the attempts told are Python's, as the Rust replay
finds them.
"""

from __future__ import annotations

import dataclasses
import unittest

from image_replay import IMAGE_GENERATE, IMAGE_WITH_OPTIONS, ImageReplays, every_case, made_wrong
from roomkit_providers.image.gemini import GeminiImage, GeminiImageConfig
from roomkit_providers.image.openai import OpenAIImage, OpenAIImageConfig

OPENAI = ImageReplays(
    vendor="openai",
    provider=OpenAIImage,
    config=OpenAIImageConfig,
    # As the Rust replay names it: the fake accepts the connection it
    # refuses, which reads as a request lost once sent.
    named={"never_connected": ["the failure's attempts differs"]},
)
GEMINI = ImageReplays(vendor="gemini", provider=GeminiImage, config=GeminiImageConfig)


class ReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_openai_image_case_replays_as_python(self) -> None:
        counts, wrong = await every_case(OPENAI)
        self.assertEqual(wrong, [], "\n".join(wrong))
        expected = {
            (IMAGE_GENERATE, "as Python"): 23,
            (IMAGE_GENERATE, "named"): 1,
            (IMAGE_WITH_OPTIONS, "as Python"): 29,
        }
        self.assertEqual(counts, expected)

    async def test_every_gemini_image_case_replays_as_python(self) -> None:
        counts, wrong = await every_case(GEMINI)
        self.assertEqual(wrong, [], "\n".join(wrong))
        expected = {(IMAGE_GENERATE, "as Python"): 44, (IMAGE_WITH_OPTIONS, "as Python"): 19}
        self.assertEqual(counts, expected)

    # The replay compares what it says it compares: each wrong refused.
    async def test_the_replay_refuses_what_python_did_not_do(self) -> None:
        def another_image(replayed):
            replayed.images[0]["data"] = replayed.images[0]["data"][:-4] + "AAAA"

        def a_move_less(replayed):
            replayed.told.pop()

        def another_status(replayed):
            replayed.error.attempts[0].status = "unknown"

        def another_body(replayed):
            seen = replayed.seen[0]
            replayed.seen[0] = dataclasses.replace(seen, body=seen.body.replace(b"fox", b"cat"))

        wrongs = [
            ("one_image", another_image, "the images"),
            ("two_images", a_move_less, "the attempts told"),
            ("refused_with_429", another_status, "the failure's attempts"),
            ("one_image", another_body, "request 0: its body"),
        ]
        for name, wrong, word in wrongs:
            found = await made_wrong(GEMINI, name, wrong)
            self.assertTrue(any(word in said for said in found), (name, found))

        def another_field(replayed):
            seen = replayed.seen[0]
            replayed.seen[0] = dataclasses.replace(seen, body=seen.body.replace(b"blue", b"pink"))

        found = await made_wrong(OPENAI, "an_edit_of_one_reference", another_field)
        self.assertTrue(any("request 0: its body" in said for said in found), found)


if __name__ == "__main__":
    unittest.main()
