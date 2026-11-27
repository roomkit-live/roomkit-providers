"""Mistral's configuration is RoomKit's MistralConfig, its provider takes
RoomKit's own configuration and names itself as RoomKit's does, and the wire
cases captured on RoomKit's Python provider replay through it."""

from __future__ import annotations

import unittest

from replay_cases import Replays, every_case
from roomkit.providers.mistral import MistralConfig as RoomKitMistralConfig
from roomkit_providers.llm.mistral import Mistral, MistralConfig

KEY = "placeholder-not-a-key"

MISTRAL = Replays(
    vendor="mistral",
    provider=Mistral,
    config=MistralConfig,
    cases="python_mistral_cases.json",
    key_header="authorization",
    crate="mistral",
)


class MistralConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for given in (
            {"api_key": KEY},
            {
                "api_key": KEY,
                "model": "mistral-small-latest",
                "reasoning_effort": "high",
                "server_url": "https://llm.example",
            },
        ):
            with self.subTest(given=sorted(given)):
                ours = MistralConfig(**given).model_dump(mode="json")
                theirs = RoomKitMistralConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_its_repr_shows_no_key(self) -> None:
        self.assertNotIn(KEY, repr(MistralConfig(api_key=KEY)))

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = Mistral(RoomKitMistralConfig(api_key=KEY))
        self.assertEqual(built.name, "MistralAIProvider")
        self.assertEqual(built.model_name, "mistral-large-latest")
        self.assertTrue(Mistral(MistralConfig(api_key=KEY)).supports_vision)


class MistralReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_python_or_is_named(self) -> None:
        counts, wrong = await every_case(MISTRAL)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 78,
                ("generate", "named"): 1,
                ("generate_structured_stream", "as Python"): 29,
                ("generate_stream", "as Python"): 3,
                ("list_models", "as Python"): 3,
                ("list_models", "named"): 1,
            },
        )


if __name__ == "__main__":
    unittest.main()
