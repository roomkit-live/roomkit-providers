#!/usr/bin/env python3
"""Write the checkout's path out of a wheel's bills of materials.

The compiler is told to write this machine's paths as names (the wheel's
build maps them), which covers the extension. The bill of materials the
build tool adds names each crate of the repository by the place it was
built from: this rewrites that place as a name, and the wheel's record of
the file with it.

    python scripts/wheel_paths.py dist/roomkit_providers-*.whl
"""

from __future__ import annotations

import base64
import hashlib
import shutil
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
# What the checkout is called in a wheel, as in its extension.
NAME = b"/roomkit-providers"


def recorded(data: bytes) -> str:
    """A file's digest and size, as a wheel's record writes them."""
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"sha256={digest},{len(data)}"


def neutral(wheel: Path, root: Path = ROOT) -> int:
    """Rewrites `wheel` with `root` written as a name in its bills of
    materials. Gives how many of them changed."""
    place = str(root).encode()
    changed: dict[str, bytes] = {}
    with zipfile.ZipFile(wheel) as archive:
        entries = [(info, archive.read(info.filename)) for info in archive.infolist()]
    for info, data in entries:
        if ".dist-info/sboms/" in info.filename and place in data:
            changed[info.filename] = data.replace(place, NAME)
    if not changed:
        return 0
    with tempfile.NamedTemporaryFile(dir=wheel.parent, suffix=".whl", delete=False) as rewritten:
        with zipfile.ZipFile(rewritten, "w", zipfile.ZIP_DEFLATED) as archive:
            for info, data in entries:
                if info.filename.endswith(".dist-info/RECORD"):
                    lines = data.decode().splitlines()
                    for at, line in enumerate(lines):
                        name = line.split(",", 1)[0]
                        if name in changed:
                            lines[at] = f"{name},{recorded(changed[name])}"
                    data = ("\n".join(lines) + "\n").encode()
                archive.writestr(info, changed.get(info.filename, data))
    shutil.move(rewritten.name, wheel)
    return len(changed)


def main() -> None:
    for name in sys.argv[1:]:
        print(f"{name}: {neutral(Path(name))} bill(s) of materials rewritten")


if __name__ == "__main__":
    main()
