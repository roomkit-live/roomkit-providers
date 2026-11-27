"""Ollama's configuration is RoomKit's OllamaConfig, its provider takes
RoomKit's own configuration and names itself as RoomKit's does, and the wire
cases captured on RoomKit's Python provider replay through it."""

from __future__ import annotations

import unittest

from replay_cases import Replays, every_case
from roomkit.providers.ollama import OllamaConfig as RoomKitOllamaConfig
from roomkit_providers.llm.ollama import Ollama, OllamaConfig

KEY = "placeholder-not-a-key"

OLLAMA = Replays(
    vendor="ollama",
    provider=Ollama,
    config=OllamaConfig,
    cases="python_ollama_cases.json",
    key_header="authorization",
    crate="ollama",
)


class OllamaConfigTests(unittest.TestCase):
    def test_the_form_is_roomkit_s(self) -> None:
        for given in (
            {},
            {"api_key": KEY, "model": "qwen3:8b", "think": "high", "keep_alive": "-1"},
            {"host": "https://ollama.example", "headers": {"X-Team": "core"}, "top_p": 1},
        ):
            with self.subTest(given=sorted(given)):
                ours = OllamaConfig(**given).model_dump(mode="json")
                theirs = RoomKitOllamaConfig(**given).model_dump(mode="json")
                self.assertEqual(list(ours), list(theirs))
                self.assertEqual(ours, theirs)

    def test_its_repr_shows_no_key_and_no_header(self) -> None:
        shown = repr(OllamaConfig(api_key=KEY, headers={"X-Proxy-Token": "t-secret-1"}))
        self.assertNotIn(KEY, shown)
        self.assertNotIn("t-secret-1", shown)

    def test_a_provider_takes_roomkit_s_configuration_and_names_itself_as_roomkit(self) -> None:
        built = Ollama(RoomKitOllamaConfig())
        self.assertEqual(built.name, "OllamaAIProvider")
        self.assertEqual(built.model_name, "llama3.2")
        self.assertTrue(Ollama(OllamaConfig()).supports_vision)


class OllamaReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_case_replays_as_python_or_is_named(self) -> None:
        counts, wrong = await every_case(OLLAMA)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 87,
                ("generate_structured_stream", "as Python"): 18,
                ("generate_stream", "as Python"): 3,
                ("list_models", "as Python"): 5,
                ("list_models", "named"): 1,
            },
        )


if __name__ == "__main__":
    unittest.main()
