"""The configurations of DeepSeek, Kimi and MiniMax are RoomKit's: the same
members, the same defaults and the same form, and the same refusal of the two
members each vendor's rule makes moot. A provider takes RoomKit's own
configuration in place of the package's."""

from __future__ import annotations

import unittest

from pydantic import ValidationError
from roomkit.providers.deepseek import DeepSeekConfig as RoomKitDeepSeekConfig
from roomkit.providers.kimi import KimiConfig as RoomKitKimiConfig
from roomkit.providers.minimax import MiniMaxConfig as RoomKitMiniMaxConfig
from roomkit_providers.llm.deepseek import DeepSeek, DeepSeekConfig
from roomkit_providers.llm.kimi import Kimi, KimiConfig
from roomkit_providers.llm.minimax import MiniMax, MiniMaxConfig

KEY = "placeholder-not-a-key"

VENDORS = [
    (DeepSeek, DeepSeekConfig, RoomKitDeepSeekConfig, "deepseek-v4-pro", "DeepSeekAIProvider"),
    (Kimi, KimiConfig, RoomKitKimiConfig, "kimi-k3.1-pro", "kimi"),
    (MiniMax, MiniMaxConfig, RoomKitMiniMaxConfig, "MiniMax-M3", "minimax"),
]


class VendorConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for _, config, roomkit, model, _ in VENDORS:
            with self.subTest(config.__name__):
                ours = config(api_key=KEY, model=model).model_dump(mode="json")
                theirs = roomkit(api_key=KEY, model=model).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_the_members_the_vendor_s_rule_decides_are_refused(self) -> None:
        for _, config, _, model, _ in VENDORS:
            for member, value in (
                ("supports_reasoning_effort_with_tools", True),
                ("thinking_field", "reasoning_content"),
            ):
                with self.subTest(config.__name__, member=member):
                    with self.assertRaises(ValidationError) as refused:
                        config(api_key=KEY, model=model, **{member: value})
                    self.assertIn(f"{config.__name__} takes no {member}", str(refused.exception))
                    self.assertNotIn(KEY, str(refused.exception))

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        for provider, config, roomkit, model, recorded in VENDORS:
            with self.subTest(provider.__name__):
                built = provider(roomkit(api_key=KEY, model=model))
                self.assertEqual(built.name, recorded)
                self.assertEqual(built.model_name, model)
                ours = provider(config(api_key=KEY, model=model))
                self.assertEqual(ours.supports_vision, built.supports_vision)


if __name__ == "__main__":
    unittest.main()
