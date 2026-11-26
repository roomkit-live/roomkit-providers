"""Capture managed llama.cpp configuration, launch arguments and Chat Completions from RoomKit."""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from unittest.mock import AsyncMock, patch

import httpx2
from roomkit import __version__
from roomkit.providers.ai import AIContext, AIMessage, AITool
from roomkit.providers.llamacpp import LlamaCppAIProvider, LlamaCppConfig
from roomkit.providers.llamacpp._builds import ASSETS, BUILD
from roomkit.providers.llamacpp.binary import LlamaBinary
from roomkit.providers.openai import OpenAIAIProvider

ROOT = Path(__file__).resolve().parents[1] / "crates/roomkit-providers-llamacpp"


def write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


async def main() -> None:
    default = LlamaCppConfig(model="example/model:Q4_K_M")
    config = LlamaCppConfig(model=default.model, enable_thinking=False, port=18080)
    sent = []

    def answer(request: httpx2.Request) -> httpx2.Response:
        sent.append(json.loads(request.content))
        return httpx2.Response(
            200,
            json={
                "id": "completion",
                "object": "chat.completion",
                "created": 0,
                "model": config.model,
                "choices": [
                    {
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": "done"},
                    }
                ],
            },
        )

    # Keep the real Python provider and SDK. Substitute only the network and child process.
    init = OpenAIAIProvider.__init__

    def local_init(provider, configuration):
        init(provider, configuration, transport=httpx2.MockTransport(answer))

    with patch.object(OpenAIAIProvider, "__init__", local_init):
        provider = LlamaCppAIProvider(config)
    try:
        with patch.object(provider._server, "start", new_callable=AsyncMock):
            for effort in (None, "high", "none"):
                await provider.generate(
                    AIContext(
                        messages=[AIMessage(role="user", content="Look up the record.")],
                        tools=[
                            AITool(
                                name="lookup",
                                description="Read a record",
                                parameters={"type": "object", "properties": {}},
                            )
                        ],
                        reasoning_effort=effort,
                    )
                )
        args = provider._server._arguments(LlamaBinary(Path("llama-server")))
        write(
            ROOT / "tests/fixtures/python.json",
            {
                "roomkit_version": __version__,
                "defaults": default.model_dump(mode="json"),
                "catalogue": provider.available_models(),
                "launch": args,
                "requests": sent,
            },
        )
    finally:
        await provider.close()
    write(ROOT / "src/builds.json", {"build": BUILD, "variants": ASSETS})


if __name__ == "__main__":
    asyncio.run(main())
