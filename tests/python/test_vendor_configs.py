"""The configurations of the vendors of OpenAI's wire are RoomKit's: the same
members, the same defaults and the same form, and the same refusal of the
members each vendor's rule makes moot. A provider takes RoomKit's own
configuration in place of the package's."""

from __future__ import annotations

import unittest

from pydantic import ValidationError
from roomkit.providers.cerebras import CerebrasConfig as RoomKitCerebrasConfig
from roomkit.providers.deepseek import DeepSeekConfig as RoomKitDeepSeekConfig
from roomkit.providers.kimi import KimiConfig as RoomKitKimiConfig
from roomkit.providers.meta import MetaConfig as RoomKitMetaConfig
from roomkit.providers.minimax import MiniMaxConfig as RoomKitMiniMaxConfig
from roomkit.providers.qwen import QwenConfig as RoomKitQwenConfig
from roomkit.providers.xai import XAIConfig as RoomKitXAIConfig
from roomkit_providers.llm.cerebras import Cerebras, CerebrasConfig
from roomkit_providers.llm.deepseek import DeepSeek, DeepSeekConfig
from roomkit_providers.llm.kimi import Kimi, KimiConfig
from roomkit_providers.llm.meta import Meta, MetaConfig
from roomkit_providers.llm.minimax import MiniMax, MiniMaxConfig
from roomkit_providers.llm.qwen import Qwen, QwenConfig
from roomkit_providers.llm.xai import XAI, XAIConfig

KEY = "placeholder-not-a-key"

VENDORS = [
    (DeepSeek, DeepSeekConfig, RoomKitDeepSeekConfig, "deepseek-v4-pro", "DeepSeekAIProvider"),
    (Kimi, KimiConfig, RoomKitKimiConfig, "kimi-k3.1-pro", "kimi"),
    (MiniMax, MiniMaxConfig, RoomKitMiniMaxConfig, "MiniMax-M3", "minimax"),
    (XAI, XAIConfig, RoomKitXAIConfig, "grok-4.7", "XAIAIProvider"),
    (Cerebras, CerebrasConfig, RoomKitCerebrasConfig, "gpt-oss-120b", "cerebras"),
    (Qwen, QwenConfig, RoomKitQwenConfig, "qwen3.7-max", "QwenAIProvider"),
    (Meta, MetaConfig, RoomKitMetaConfig, "muse-spark-1.3", "MetaAIProvider"),
]
# The vendors whose provider sends reasoning where the vendor reads it, and
# whose configuration takes no thinking field.
NAMING_THEIR_FIELD = {DeepSeekConfig, KimiConfig, MiniMaxConfig, CerebrasConfig}


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
            decided = [("supports_reasoning_effort_with_tools", True)]
            if config in NAMING_THEIR_FIELD:
                decided.append(("thinking_field", "reasoning_content"))
            for member, value in decided:
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

    def test_a_thinking_field_is_taken_where_roomkit_s_configuration_takes_one(self) -> None:
        for _, config, roomkit, model, _ in VENDORS:
            if config in NAMING_THEIR_FIELD:
                continue
            with self.subTest(config.__name__):
                stated = {"api_key": KEY, "model": model, "thinking_field": "reasoning"}
                ours = config(**stated).model_dump(mode="json")
                self.assertEqual(ours, roomkit(**stated).model_dump(mode="json"))

    def test_a_model_left_out_is_roomkit_s_default(self) -> None:
        for config, roomkit in ((XAIConfig, RoomKitXAIConfig), (MetaConfig, RoomKitMetaConfig)):
            with self.subTest(config.__name__):
                self.assertEqual(config(api_key=KEY).model, roomkit(api_key=KEY).model)

    def test_meta_refuses_a_key_no_header_can_carry_without_saying_it(self) -> None:
        key = "sk-secret\u2026"
        for config in (MetaConfig, RoomKitMetaConfig):
            with self.subTest(config.__name__):
                with self.assertRaises(ValidationError) as refused:
                    config(api_key=key)
                self.assertIn("non-ASCII", str(refused.exception))
                self.assertNotIn("sk-secret", str(refused.exception))


if __name__ == "__main__":
    unittest.main()
