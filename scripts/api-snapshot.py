#!/usr/bin/env python3
"""Rewrite the public Python API snapshot from a freshly built wheel.

Builds the package, installs it in a new environment beside the RoomKit of
the interpreter that runs this, and writes its snapshot with
tests/python/api_signatures.py into tests/python/. The Python tests fail
while the snapshot differs from the code.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

import roomkit_env

MATURIN = tuple(
    shlex.split(os.environ.get("MATURIN", "uv tool run --from 'maturin>=1.15,<2' maturin"))
)


def run(*arguments: str, cwd: Path) -> None:
    subprocess.run(arguments, cwd=cwd, check=True)


def build(root: Path, out: Path) -> Path:
    run(
        *MATURIN,
        "build",
        "--locked",
        "--interpreter",
        sys.executable,
        "--out",
        str(out),
        cwd=root,
    )
    (wheel,) = out.glob("*.whl")
    return wheel


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--wheel", type=Path, help="a prebuilt wheel of the package")
    options = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="roomkit-providers-api-") as temporary:
        work = Path(temporary)
        wheel = options.wheel.resolve() if options.wheel else build(root, work / "wheels")
        requirement = roomkit_env.requirement()
        if requirement is None:
            sys.exit(f"RoomKit is not installed for {sys.executable}: the package depends on it")
        environment = work / "venv"
        run("uv", "venv", "--python", sys.executable, str(environment), cwd=work)
        python = environment / "bin" / "python"
        run("uv", "pip", "install", "--python", str(python), str(wheel), requirement, cwd=work)
        writer = str(root / "tests/python/api_signatures.py")
        directory = str(root / "tests/python")
        run(str(python), "-I", writer, "write", "roomkit_providers", directory, cwd=work)


if __name__ == "__main__":
    main()
