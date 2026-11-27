#!/usr/bin/env python3
"""Write the model catalogues of RoomKit's Python, for the OpenAI crate.

One file for OpenAI and one for each vendor the crate serves on the same
wire: DeepSeek, Kimi, MiniMax, xAI, Cerebras, Qwen and Meta. A provider reads from its vendor's file
what the vendor's own endpoint takes of each model: for OpenAI, whether Chat
Completions serves it, whether it takes function tools there and what
reasoning effort a turn with tools carries; for every vendor, the least
effort a model takes and whether it reads images. A file is each model of
the vendor's ``MODELS`` dumped with ``model_dump(mode="json")``, read by the
contract's ``ModelInfo``.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-openai-catalogue``).
"""

from __future__ import annotations

import json
from pathlib import Path

import roomkit
from roomkit.providers.cerebras.models import MODELS as CEREBRAS
from roomkit.providers.deepseek.models import MODELS as DEEPSEEK
from roomkit.providers.kimi.models import MODELS as KIMI
from roomkit.providers.meta.models import MODELS as META
from roomkit.providers.minimax.models import MODELS as MINIMAX
from roomkit.providers.openai.models import MODELS as OPENAI
from roomkit.providers.qwen.models import MODELS as QWEN
from roomkit.providers.xai.models import MODELS as XAI

REPOSITORY = Path(__file__).resolve().parent.parent
CATALOGUES = REPOSITORY / "crates/roomkit-providers-openai/catalog"

# The file of each vendor's catalogue. OpenAI's keeps the name it had when
# it was the crate's only one.
FILES = {
    "models.json": OPENAI,
    "deepseek.json": DEEPSEEK,
    "kimi.json": KIMI,
    "minimax.json": MINIMAX,
    "xai.json": XAI,
    "cerebras.json": CEREBRAS,
    "qwen.json": QWEN,
    "meta.json": META,
}


def main() -> None:
    CATALOGUES.mkdir(parents=True, exist_ok=True)
    for name, models in FILES.items():
        catalogue = {
            "generator": "scripts/generate_openai_catalogue.py",
            "roomkit_version": roomkit.__version__,
            "models": [model.model_dump(mode="json") for model in models],
        }
        written = CATALOGUES / name
        written.write_text(json.dumps(catalogue, indent=1) + "\n", encoding="utf-8")
        print(
            f"Wrote {written.relative_to(REPOSITORY)}: {len(models)} models, "
            f"from roomkit {roomkit.__version__}"
        )


if __name__ == "__main__":
    main()
