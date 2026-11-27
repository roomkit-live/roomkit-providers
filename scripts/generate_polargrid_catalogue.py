#!/usr/bin/env python3
"""Write PolarGrid's model catalogue of RoomKit's Python, for the PolarGrid crate.

The crate's provider answers its catalogue from it, and its listing fills
what the endpoint leaves out (the context window, the display name) from it.
The file is each model of ``roomkit.providers.polargrid.models.MODELS`` dumped
with ``model_dump(mode="json")``, read by the contract's ``ModelInfo``.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-polargrid-catalogue``).
"""

from __future__ import annotations

import json
from pathlib import Path

import roomkit
from polargrid.client import DEFAULT_REGION, POLARGRID_REGIONS
from roomkit.providers.polargrid.models import CURATED_BY_ID, MODELS, REGION_ALIASES, REGIONS

REPOSITORY = Path(__file__).resolve().parent.parent
CATALOGUE = REPOSITORY / "crates/roomkit-providers-polargrid/catalog/models.json"


def main() -> None:
    catalogue = {
        "generator": "scripts/generate_polargrid_catalogue.py",
        "roomkit_version": roomkit.__version__,
        "models": [model.model_dump(mode="json") for model in MODELS],
        # Every model the provider knows, a pilot and a speech model among
        # them: what a listing backfills its entries from.
        "curated": [model.model_dump(mode="json") for model in CURATED_BY_ID.values()],
        "regions": [region.model_dump(mode="json") for region in REGIONS],
        # The friendly names of a region a configuration may pin, by the id
        # they stand for (the SDK's own table, which RoomKit mirrors).
        "aliases": REGION_ALIASES,
        # The edges polargrid-sdk knows, by id: the URL a pinned region is
        # sent to and the name it reports, and the one it falls back to when
        # the autorouter picks none.
        "edges": {
            region: {"url": edge["url"], "name": edge["name"]}
            for region, edge in POLARGRID_REGIONS.items()
        },
        "fallback": DEFAULT_REGION,
    }
    CATALOGUE.parent.mkdir(parents=True, exist_ok=True)
    CATALOGUE.write_text(json.dumps(catalogue, indent=1) + "\n", encoding="utf-8")
    print(
        f"Wrote {CATALOGUE.relative_to(REPOSITORY)}: {len(MODELS)} models, "
        f"{len(CURATED_BY_ID)} curated, {len(REGIONS)} regions, "
        f"from roomkit {roomkit.__version__}"
    )


if __name__ == "__main__":
    main()
