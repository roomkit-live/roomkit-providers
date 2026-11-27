"""DeepSeek's, Kimi's and MiniMax's recorded wire cases, replayed through the
package's providers.

Each case of ``python_<vendor>_cases.json``, captured on RoomKit's own
provider of the vendor, is asked of the package's provider as RoomKit asks
it, against a local fake of the endpoint. What the provider sent and gave is
what the Rust provider sends and gives, which is what Python did: the Rust
replay names none of these cases.
"""

from __future__ import annotations

import unittest

from replay_cases import Replays, every_case
from roomkit_providers.llm.deepseek import DeepSeek, DeepSeekConfig
from roomkit_providers.llm.kimi import Kimi, KimiConfig
from roomkit_providers.llm.minimax import MiniMax, MiniMaxConfig


def of_openai_s_wire(vendor: str, provider: type, config: type) -> Replays:
    """A vendor of OpenAI's wire: its fixture among the OpenAI crate's, no
    case named."""
    return Replays(
        vendor=vendor,
        provider=provider,
        config=config,
        cases=f"python_{vendor}_cases.json",
        key_header="authorization",
        crate="openai",
        named_cases=None,
    )


DEEPSEEK = of_openai_s_wire("deepseek", DeepSeek, DeepSeekConfig)
KIMI = of_openai_s_wire("kimi", Kimi, KimiConfig)
MINIMAX = of_openai_s_wire("minimax", MiniMax, MiniMaxConfig)


class ReplayTests(unittest.IsolatedAsyncioTestCase):
    async def test_every_deepseek_case_replays_as_python(self) -> None:
        counts, wrong = await every_case(DEEPSEEK)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 49,
                ("generate_structured_stream", "as Python"): 1,
                ("list_models", "as Python"): 1,
            },
        )

    async def test_every_kimi_case_replays_as_python(self) -> None:
        counts, wrong = await every_case(KIMI)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 42,
                ("generate_structured_stream", "as Python"): 2,
                ("list_models", "as Python"): 1,
            },
        )

    async def test_every_minimax_case_replays_as_python(self) -> None:
        counts, wrong = await every_case(MINIMAX)
        self.assertEqual(wrong, [], "\n".join(wrong))
        self.assertEqual(
            counts,
            {
                ("generate", "as Python"): 47,
                ("generate_structured_stream", "as Python"): 1,
                ("list_models", "as Python"): 1,
            },
        )


if __name__ == "__main__":
    unittest.main()
