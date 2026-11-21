#!/usr/bin/env python3
"""Write Anthropic's model catalogue of RoomKit's Python, for the Anthropic crate.

The crate's provider reads from it what Anthropic's own endpoint takes of each
model: whether it holds a tool declared and unseen, which reasoning efforts it
takes, how its reasoning is turned off, the least effort it takes, and whether
it takes images. The file is each model of
``roomkit.providers.anthropic.models.MODELS`` dumped with
``model_dump(mode="json")``, read by the contract's ``ModelInfo``.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-anthropic-catalogue``).
"""

from __future__ import annotations

import json
from pathlib import Path

import roomkit
from roomkit.providers.anthropic.models import MODELS

REPOSITORY = Path(__file__).resolve().parent.parent
CATALOGUE = REPOSITORY / "crates/roomkit-providers-anthropic/catalog/models.json"


def main() -> None:
    catalogue = {
        "generator": "scripts/generate_anthropic_catalogue.py",
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
