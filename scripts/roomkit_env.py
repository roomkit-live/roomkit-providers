"""The RoomKit of the environment that runs a check, for an environment the
check builds: the adapter is tested against the RoomKit the developer has."""

from __future__ import annotations

import json
import sys
from importlib import metadata


def requirement() -> str | None:
    """What installs the RoomKit this interpreter has: the directory or the
    URL it was installed from, else its version from the index. None when
    RoomKit is not installed."""
    try:
        roomkit = metadata.distribution("roomkit")
    except metadata.PackageNotFoundError:
        return None
    origin = roomkit.read_text("direct_url.json")
    if origin is not None:
        return f"roomkit @ {json.loads(origin)['url']}"
    return f"roomkit=={roomkit.version}"


if __name__ == "__main__":
    # For the Makefile: succeeds where this interpreter has RoomKit.
    sys.exit(requirement() is None)
