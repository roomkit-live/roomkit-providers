#!/usr/bin/env python3
"""Write Mistral's model catalogue of RoomKit's Python, for the Mistral crate.

The crate's provider answers its catalogue from it, and its listing fills
what the endpoint leaves out (the context window, the display name) from it.
The file is each model of ``roomkit.providers.mistral.models.MODELS`` dumped
with ``model_dump(mode="json")``, read by the contract's ``ModelInfo``.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-mistral-catalogue``).
"""

from __future__ import annotations

import json
from pathlib import Path

import roomkit
from roomkit.providers.mistral.models import MODELS

REPOSITORY = Path(__file__).resolve().parent.parent
CATALOGUE = REPOSITORY / "crates/roomkit-providers-mistral/catalog/models.json"


def main() -> None:
    catalogue = {
        "generator": "scripts/generate_mistral_catalogue.py",
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
