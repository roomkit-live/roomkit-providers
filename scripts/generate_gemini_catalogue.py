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
from roomkit.providers.gemini.image_models import MODELS as IMAGE_MODELS
from roomkit.providers.gemini.models import MODELS

REPOSITORY = Path(__file__).resolve().parent.parent
CATALOGUES = REPOSITORY / "crates/roomkit-providers-gemini/catalog"

# The conversational models, and the image models, disjoint from them (RFC
# §25.6): each an ImageModelInfo, its image capabilities with it.
FILES = {"models.json": MODELS, "images.json": IMAGE_MODELS}


def main() -> None:
    CATALOGUES.mkdir(parents=True, exist_ok=True)
    for name, models in FILES.items():
        catalogue = {
            "generator": "scripts/generate_gemini_catalogue.py",
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
