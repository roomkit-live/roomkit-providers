#!/usr/bin/env python3
"""Write Gemini's model catalogue of RoomKit's Python, for the Gemini crate.

The crate's provider reads from it what the Developer API takes of each
model: the thinking levels it takes, minimal among them, whether it cannot
stop reasoning, and whether it takes images; its listing fills what the
endpoint leaves out from it. The file is each model of
``roomkit.providers.gemini.models.MODELS`` dumped with
``model_dump(mode="json")``, read by the contract's ``ModelInfo``.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-gemini-catalogue``).
"""

from __future__ import annotations

import json
from pathlib import Path

import roomkit
from roomkit.providers.gemini.models import MODELS

REPOSITORY = Path(__file__).resolve().parent.parent
CATALOGUE = REPOSITORY / "crates/roomkit-providers-gemini/catalog/models.json"


def main() -> None:
    catalogue = {
        "generator": "scripts/generate_gemini_catalogue.py",
        "roomkit_version": roomkit.__version__,
        "models": [model.model_dump(mode="json") for model in MODELS],
    }
    CATALOGUE.parent.mkdir(parents=True, exist_ok=True)
    CATALOGUE.write_text(json.dumps(catalogue, indent=1) + "\n", encoding="utf-8")
    print(
        f"Wrote {CATALOGUE.relative_to(REPOSITORY)}: {len(MODELS)} models, "
        f"from roomkit {roomkit.__version__}"
    )


if __name__ == "__main__":
    main()
