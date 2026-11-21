"""The RoomKit of the environment that runs a check, for an environment the
check builds: the package is tested against the RoomKit the developer has."""

from __future__ import annotations

import json
from importlib import metadata


def requirement() -> str | None:
    """What installs the RoomKit this interpreter has: the directory or the
    URL it was installed from, else its version from the index. None when
    RoomKit is not installed."""
    try:
        roomkit = metadata.distribution("roomkit")
    except metadata.PackageNotFoundError:
        return None
    written = roomkit.read_text("direct_url.json")
    if written is None:
        return f"roomkit=={roomkit.version}"
    origin = json.loads(written)
    url = origin["url"]
    source = origin.get("vcs_info")
    if source is not None:
        # A repository, at the commit that was installed.
        url = f"{source['vcs']}+{url}@{source['commit_id']}"
    return f"roomkit @ {url}"
