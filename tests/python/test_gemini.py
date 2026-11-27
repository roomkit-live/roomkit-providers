"""Gemini's configuration is RoomKit's GeminiConfig, its provider takes
RoomKit's own configuration and names itself as RoomKit's does, and the wire
cases captured on RoomKit's Python provider replay through it."""

from __future__ import annotations

import unittest

from replay_cases import Replays, every_case
from roomkit.providers.gemini import GeminiConfig as RoomKitGeminiConfig
from roomkit_providers.llm.gemini import Gemini, GeminiConfig

KEY = "placeholder-not-a-key"

GEMINI = Replays(
    vendor="gemini",
    provider=Gemini,
    config=GeminiConfig,
    cases="python_gemini_cases.json",
    key_header="x-goog-api-key",
    crate="gemini",
)


class GeminiConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for given in (
            {"api_key": KEY},
            {"api_key": KEY, "model": "gemini-2.5-flash", "thinking_level": "low", "timeout": 5},
        ):
            with self.subTest(given=sorted(given)):
                ours = GeminiConfig(**given).model_dump(mode="json")
                theirs = RoomKitGeminiConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_its_repr_shows_no_key(self) -> None:
        self.assertNotIn(KEY, repr(GeminiConfig(api_key=KEY)))

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = Gemini(RoomKitGeminiConfig(api_key=KEY))
        self.assertEqual(built.name, "GeminiAIProvider")
        self.assertEqual(built.model_name, "gemini-3.8-flash")
        self.assertTrue(Gemini(GeminiConfig(api_key=KEY)).supports_vision)


class GeminiReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_python_or_is_named(self) -> None:
        counts, wrong = await every_case(GEMINI)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 110,
                ("generate_structured_stream", "as Python"): 6,
                ("generate_stream", "as Python"): 1,
                ("list_models", "as Python"): 4,
                ("list_models", "named"): 1,
            },
        )


if __name__ == "__main__":
    unittest.main()
