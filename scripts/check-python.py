#!/usr/bin/env python3
"""Build a wheel, inspect its contents and test an isolated installation.

The package depends on RoomKit: each artifact is installed in a new
environment beside the RoomKit of the interpreter that runs this, which
must have one.

The wheel this script builds carries the scripted provider the tests of the
generic layer are written against (the binding's `testing` feature). The
source distribution is installed as it is distributed, without it, and so
is a wheel given with --wheel: those tests are skipped there, and the
artifact is checked not to carry it.
"""

from __future__ import annotations

import argparse
import os
import re
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
import wheel_paths

PACKAGE = "roomkit_providers"
NOTICES = f"{PACKAGE}/licenses/native_dependencies.txt"
# The build tool: MATURIN names another command for it, a local installation
# for one.
MATURIN = tuple(
    shlex.split(os.environ.get("MATURIN", "uv tool run --from 'maturin>=1.15,<2' maturin"))
)
# What runs the tests and the examples with no network but loopback: one
# that lost its way to its local fake fails there, and reaches no service.
NO_NETWORK = "scripts/no-network.sh"
# The examples an installation runs, each checking what it shows.
EXAMPLES = [("local_server.py",), ("roomkit_agent.py",), ("anthropic_agent.py",)]
# What builds the scripted provider into a wheel, beside the features
# pyproject.toml names, which a command's own replace.
TESTING = "pyo3/extension-module,testing"


def run(*arguments: str, cwd: Path) -> None:
    subprocess.run(arguments, cwd=cwd, check=True)


def check_notices(wheel: zipfile.ZipFile, root: Path, work: Path) -> None:
    """The wheel's notices cover every locked crate linked into the extension."""
    assert NOTICES in wheel.namelist(), "the wheel has no native dependency notices"
    notices = work / "native_dependencies.txt"
    notices.write_bytes(wheel.read(NOTICES))
    run(sys.executable, str(root / "scripts/native-notices.py"), "check", str(notices), cwd=root)


def build_wheel(root: Path, work: Path) -> Path:
    """The wheel the tests install, the scripted provider in it, built as
    one that is distributed: the paths of this machine written as names."""
    wheels = work / "wheels"
    # A later mapping wins: the registry's and the checkout's do under the
    # home directory. The C a crate compiles is told the same as the Rust.
    named = {Path.home(): "/home", Path.home() / ".cargo": "/cargo", root: "/roomkit-providers"}
    flags = {"RUSTFLAGS": "--remap-path-prefix", "CFLAGS": "-ffile-prefix-map"}
    mapped = {
        variable: " ".join(
            [
                os.environ.get(variable, ""),
                *(f"{flag}={path}={name}" for path, name in named.items()),
            ]
        ).strip()
        for variable, flag in flags.items()
    }
    command = [*MATURIN, "build", "--locked", "--features", TESTING]
    command += ["--interpreter", sys.executable, "--out", str(wheels)]
    subprocess.run(command, cwd=root, check=True, env=os.environ | mapped)
    for built in wheels.glob("*.whl"):
        wheel_paths.neutral(built, root)
    (wheel,) = wheels.glob("*.whl")
    return wheel


def build_sdist(root: Path, work: Path) -> Path:
    # An sdist must independently rebuild the mixed package, including path
    # dependencies, from the repository's files only.
    sources = work / "sources"
    run(*MATURIN, "sdist", "--out", str(sources), cwd=root)
    (source,) = sources.glob("*.tar.gz")
    return source


def requirement(written: str) -> tuple[str, list[str], str]:
    """A requirement's name, its version clauses and its marker, however
    they are spaced and ordered."""
    clauses, _, marker = written.partition(";")
    clauses = clauses.replace(" ", "")
    name = re.match(r"[A-Za-z0-9._-]+", clauses)
    assert name is not None, written
    versions = sorted(clause for clause in clauses[name.end() :].split(",") if clause)
    return name.group().lower(), versions, marker.strip()


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
    assert sorted(map(requirement, dependencies)) == sorted(
        map(requirement, project["dependencies"])
    ), f"the wheel depends on what the project does not declare: {dependencies}"


def check_paths(archive: zipfile.ZipFile, root: Path) -> None:
    """The wheel names no place of the machine that built it: neither a
    home directory's registry of crates, nor this machine's home or
    checkout."""
    told = [b"/.cargo/registry/", str(Path.home()).encode(), str(root).encode()]
    for name in archive.namelist():
        held = archive.read(name)
        for path in told:
            assert path not in held, f"{name} holds a path of its builder: {path.decode()}"


def check_wheel(wheel: Path, root: Path, work: Path) -> None:
    """The wheel holds the extension, typing marker, notices and metadata, nothing else."""
    with zipfile.ZipFile(wheel) as archive:
        check_paths(archive, root)
        names = archive.namelist()
        assert any(name.endswith((".so", ".pyd")) for name in names), names
        assert f"{PACKAGE}/py.typed" in names, names
        assert f"{PACKAGE}/_native.pyi" in names, names
        check_notices(archive, root, work)
        metadata = archive.read(next(n for n in names if n.endswith("/METADATA")))
        check_metadata(metadata, names, root)
    # Ignored local files must not reach the wheel.
    artifact_files.check_wheel(wheel, root, PACKAGE)


def run_tests(python: Path, root: Path, work: Path) -> None:
    subprocess.run(
        [
            str(root / NO_NETWORK),
            str(python),
            "-X",
            "faulthandler",
            "-I",
            "-m",
            "unittest",
            "discover",
            "-s",
            str(root / "tests/python"),
            "-v",
        ],
        cwd=work,
        check=True,
    )


def run_examples(python: Path, root: Path, work: Path) -> None:
    for script, *arguments in EXAMPLES:
        example = (str(python), "-I", str(root / "examples" / script), *arguments)
        run(str(root / NO_NETWORK), *example, cwd=work)


def check_scripted(python: Path, work: Path, expected: bool) -> None:
    """The scripted provider is in the wheel built for the tests, which
    then skip nothing, and in no artifact as it is distributed."""
    has = "from roomkit_providers import _native; print(hasattr(_native, '_scripted'))"
    found = subprocess.check_output([str(python), "-I", "-c", has], cwd=work, text=True).strip()
    assert found == str(expected), f"the scripted provider: expected {expected}, found {found}"


def install_and_test(label: str, package: Path, scripted: bool, root: Path, work: Path) -> None:
    """Install one artifact in a new environment, beside the RoomKit of the
    interpreter that runs this, and test it: the tests, then the examples a
    user would run."""
    requirement = roomkit_env.requirement()
    if requirement is None:
        sys.exit(f"RoomKit is not installed for {sys.executable}: the package is tested with it")
    environment = work / label
    run("uv", "venv", "--python", sys.executable, str(environment), cwd=work)
    python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    run("uv", "pip", "install", "--python", str(python), str(package), requirement, cwd=work)
    check_scripted(python, work, scripted)
    run_tests(python, root, work)
    run_examples(python, root, work)


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
    options = parser.parse_args()
    if bool(options.wheel) != bool(options.sdist):
        parser.error("--wheel and --sdist must be supplied together")
    with tempfile.TemporaryDirectory(prefix="roomkit-providers-python-") as temporary:
        work = Path(temporary)
        wheel = options.wheel.resolve() if options.wheel else build_wheel(root, work)
        check_wheel(wheel, root, work)
        source = options.sdist.resolve() if options.sdist else build_sdist(root, work)
        artifact_files.check_sdist(source, root)
        # Validate both artifacts independently.
        install_and_test("wheel", wheel, not options.wheel, root, work)
        install_and_test("sdist", source, False, root, work)


if __name__ == "__main__":
    main()
