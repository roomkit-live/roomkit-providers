#!/usr/bin/env python3
"""Build a wheel, inspect its contents and test an isolated installation.

With --roomkit, test the RoomKit adapter instead: the wheel installed with
its `roomkit` extra beside the RoomKit of the interpreter that runs this.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
import tempfile
import tomllib
import zipfile
from email.parser import BytesParser
from pathlib import Path

import artifact_files
import roomkit_env

PACKAGE = "roomkit_providers"
NOTICES = f"{PACKAGE}/licenses/native_dependencies.txt"
# The build tool: MATURIN names another command for it, a local installation
# for one.
MATURIN = tuple(
    shlex.split(os.environ.get("MATURIN", "uv tool run --from 'maturin>=1.15,<2' maturin"))
)
# The examples a core installation runs, each checking what it shows, and
# those an installation with RoomKit runs.
EXAMPLES = [("local_server.py",)]
ROOMKIT_EXAMPLES = [("roomkit_agent.py",)]


def run(*arguments: str, cwd: Path) -> None:
    subprocess.run(arguments, cwd=cwd, check=True)


def check_notices(wheel: zipfile.ZipFile, root: Path, work: Path) -> None:
    """The wheel's notices cover every locked crate linked into the extension."""
    assert NOTICES in wheel.namelist(), "the wheel has no native dependency notices"
    notices = work / "native_dependencies.txt"
    notices.write_bytes(wheel.read(NOTICES))
    run(sys.executable, str(root / "scripts/native-notices.py"), "check", str(notices), cwd=root)


def build_wheel(root: Path, work: Path) -> Path:
    wheels = work / "wheels"
    run(
        *MATURIN,
        "build",
        "--locked",
        "--interpreter",
        sys.executable,
        "--out",
        str(wheels),
        cwd=root,
    )
    (wheel,) = wheels.glob("*.whl")
    return wheel


def build_sdist(root: Path, work: Path) -> Path:
    # An sdist must independently rebuild the mixed package, including path
    # dependencies, from the repository's files only.
    sources = work / "sources"
    run(*MATURIN, "sdist", "--out", str(sources), cwd=root)
    (source,) = sources.glob("*.tar.gz")
    return source


def check_metadata(metadata: bytes, names: list[str], root: Path) -> None:
    """The wheel declares the version, description, licenses and links of the project."""
    project = tomllib.loads((root / "pyproject.toml").read_text())["project"]
    message = BytesParser().parsebytes(metadata)
    assert message["Version"] == project["version"], message["Version"]
    assert message["Description-Content-Type"].split(";", 1)[0] == "text/markdown"
    assert (
        message.get_payload(decode=True).decode("utf-8").strip()
        == (root / "README.md").read_text(encoding="utf-8").strip()
    )
    assert message["License-Expression"] == project["license"], message["License-Expression"]
    license_files = message.get_all("License-File", [])
    assert set(license_files) == set(project["license-files"]), license_files
    assert all(
        any(name.endswith(f".dist-info/licenses/{license_file}") for name in names)
        for license_file in license_files
    ), "Declared licenses must be included in the wheel"
    assert set(message.get_all("Project-URL", [])) == {
        f"{label}, {url}" for label, url in project["urls"].items()
    }
    dependencies = message.get_all("Requires-Dist", [])
    assert all(
        dependency.partition(";")[2].strip().replace("'", '"') == 'extra == "roomkit"'
        for dependency in dependencies
    ), "Core installation must have no Python dependencies"


def check_wheel(wheel: Path, root: Path, work: Path) -> None:
    """The wheel holds the extension, typing marker, notices and metadata, nothing else."""
    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        assert any(name.endswith((".so", ".pyd")) for name in names), names
        assert f"{PACKAGE}/py.typed" in names, names
        assert f"{PACKAGE}/_native.pyi" in names, names
        check_notices(archive, root, work)
        metadata = archive.read(next(n for n in names if n.endswith("/METADATA")))
        check_metadata(metadata, names, root)
    # Ignored local files must not reach the wheel.
    artifact_files.check_wheel(wheel, root, PACKAGE)


def run_tests(python: Path, root: Path, work: Path, tests: str = "tests/python") -> None:
    subprocess.run(
        [
            str(python),
            "-X",
            "faulthandler",
            "-I",
            "-m",
            "unittest",
            "discover",
            "-s",
            str(root / tests),
            "-v",
        ],
        cwd=work,
        check=True,
    )


def run_examples(python: Path, root: Path, work: Path, examples: list[tuple[str, ...]]) -> None:
    for script, *arguments in examples:
        run(str(python), "-I", str(root / "examples" / script), *arguments, cwd=work)


def install_and_test(label: str, package: Path, root: Path, work: Path) -> None:
    """Install one artifact alone, with no other package, and test it: the
    tests, then the example a user would run."""
    environment = work / label
    run("uv", "venv", "--python", sys.executable, str(environment), cwd=work)
    python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    run("uv", "pip", "install", "--python", str(python), "--no-deps", str(package), cwd=work)
    installed = subprocess.check_output(
        ["uv", "pip", "list", "--python", str(python), "--format", "freeze"], cwd=work, text=True
    )
    names = {line.partition("==")[0].lower() for line in installed.splitlines() if line}
    assert names == {"roomkit-providers"}, f"the environment holds more than the package: {names}"
    run_tests(python, root, work)
    run_examples(python, root, work, EXAMPLES)


def test_with_roomkit(wheel: Path, root: Path, work: Path) -> None:
    """Install the wheel with its `roomkit` extra beside the RoomKit this
    interpreter has, and test the adapter: its tests, then its example."""
    requirement = roomkit_env.requirement()
    if requirement is None:
        sys.exit(f"RoomKit is not installed for {sys.executable}: the adapter is tested against it")
    environment = work / "roomkit"
    run("uv", "venv", "--python", sys.executable, str(environment), cwd=work)
    python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    extra = f"{wheel}[roomkit]"
    run("uv", "pip", "install", "--python", str(python), extra, requirement, cwd=work)
    run_tests(python, root, work, "tests/python/roomkit")
    run_examples(python, root, work, ROOMKIT_EXAMPLES)


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    # Extracted sdists live outside the checkout's rustup override.
    os.environ.setdefault(
        "RUSTUP_TOOLCHAIN",
        tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"],
    )
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", type=Path, help="Validate this prebuilt wheel")
    parser.add_argument("--sdist", type=Path, help="Validate this source distribution")
    parser.add_argument("--roomkit", action="store_true", help="Test the RoomKit adapter")
    options = parser.parse_args()
    if not options.roomkit and bool(options.wheel) != bool(options.sdist):
        parser.error("--wheel and --sdist must be supplied together")
    with tempfile.TemporaryDirectory(prefix="roomkit-providers-python-") as temporary:
        work = Path(temporary)
        wheel = options.wheel.resolve() if options.wheel else build_wheel(root, work)
        if options.roomkit:
            test_with_roomkit(wheel, root, work)
            return
        check_wheel(wheel, root, work)
        source = options.sdist.resolve() if options.sdist else build_sdist(root, work)
        artifact_files.check_sdist(source, root)
        # Validate both artifacts independently.
        for label, package in [("wheel", wheel), ("sdist", source)]:
            install_and_test(label, package, root, work)


if __name__ == "__main__":
    main()
