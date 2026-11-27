"""PolarGrid's configuration is RoomKit's PolarGridConfig, its provider takes
RoomKit's own configuration and names itself as RoomKit's does, and the wire
cases captured on RoomKit's Python provider replay through it."""

from __future__ import annotations

import unittest

from pydantic import ValidationError
from replay_cases import Replays, every_case
from roomkit.providers.polargrid import PolarGridConfig as RoomKitPolarGridConfig
from roomkit_providers.llm.polargrid import PolarGrid, PolarGridConfig

KEY = "placeholder-not-a-key"

POLARGRID = Replays(
    vendor="polargrid",
    provider=PolarGrid,
    config=PolarGridConfig,
    cases="python_polargrid_cases.json",
    key_header="authorization",
    crate="polargrid",
)


class PolarGridConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for given in (
            {"api_key": KEY},
            {"api_key": KEY, "region": "Montreal", "max_tokens": 512, "thinking": True},
            {"api_key": KEY, "region": "yto-01", "top_p": 0.5, "max_retries": 2},
        ):
            with self.subTest(given=sorted(given)):
                ours = PolarGridConfig(**given).model_dump(mode="json")
                theirs = RoomKitPolarGridConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_an_edge_polargrid_does_not_run_and_a_bound_under_one_are_refused(self) -> None:
        for given in ({"region": "yul-2"}, {"max_tokens": 0}):
            with self.subTest(given=given):
                with self.assertRaises(ValidationError):
                    RoomKitPolarGridConfig(api_key=KEY, **given)
                with self.assertRaises(ValidationError):
                    PolarGridConfig(api_key=KEY, **given)

    def test_its_repr_shows_no_key(self) -> None:
        self.assertNotIn(KEY, repr(PolarGridConfig(api_key=KEY)))

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = PolarGrid(RoomKitPolarGridConfig(api_key=KEY, region="toronto"))
        self.assertEqual(built.name, "PolarGridAIProvider")
        self.assertEqual(built.model_name, "qwen-3.8-27b")
        self.assertFalse(PolarGrid(PolarGridConfig(api_key=KEY)).supports_vision)


class PolarGridReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_python_or_is_named(self) -> None:
        counts, wrong = await every_case(POLARGRID)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 68,
                ("generate_structured_stream", "as Python"): 23,
                ("generate_stream", "as Python"): 2,
                ("list_models", "as Python"): 5,
            },
        )


if __name__ == "__main__":
    unittest.main()
