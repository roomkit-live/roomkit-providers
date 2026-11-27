"""vLLM's configuration is RoomKit's VLLMConfig: the same members, the same
defaults and the same form. A provider takes RoomKit's own configuration in
place of the package's, as RoomKit's create_vllm_provider takes it."""

from __future__ import annotations

import unittest

from roomkit.providers.vllm import VLLMConfig as RoomKitVLLMConfig
from roomkit_providers.llm.vllm import VLLM, VLLMConfig

KEY = "placeholder-not-a-key"

GIVEN = [
    {"model": "qwen3-8b"},
    {
        "model": "qwen3-8b",
        "api_key": KEY,
        "headers": {"X-Route": "blue"},
        "top_p": 1,
        "top_k": -1,
        "min_p": 0.05,
        "enable_thinking": False,
        "reasoning_effort": "low",
        "extra_body": {"chat_template_kwargs": {"custom": 1}},
    },
]


class VLLMConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for given in GIVEN:
            with self.subTest(given=sorted(given)):
                ours = VLLMConfig(**given).model_dump(mode="json")
                theirs = RoomKitVLLMConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_its_repr_shows_no_key_and_no_header_s_value(self) -> None:
        shown = repr(VLLMConfig(**GIVEN[1]))
        self.assertNotIn(KEY, shown)
        self.assertNotIn("blue", shown)

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = VLLM(RoomKitVLLMConfig(model="qwen3-8b"))
        self.assertEqual(built.name, "vllm")
        self.assertEqual(built.model_name, "qwen3-8b")
        self.assertTrue(VLLM(VLLMConfig(model="qwen3-8b")).supports_vision)


if __name__ == "__main__":
    unittest.main()
