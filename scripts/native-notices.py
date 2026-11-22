#!/usr/bin/env python3
"""Write or check the third-party notices bundled with the native Python package.

License texts come from the locked Cargo registry sources, including license
files nested in a package; no notice is fetched from the network. Run from any
directory after a dependency change:

    python scripts/native-notices.py write
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
NOTICE_PREFIXES = ("license", "copying", "copyright", "notice")

MANIFEST = "bindings/python/Cargo.toml"
OUTPUT = "python/roomkit_providers/licenses/native_dependencies.txt"
# The licenses of the repository's own code, written first in the wheel's.
OWN_LICENSES = ("MIT", "Unicode-3.0")
TITLE = (
    "Third-party notices for the roomkit-providers native extension.\n\n"
    "License texts from locked dependency sources.\n"
)
# For packages with no notice file, keyed by a "name-version" glob: files of
# the package source that hold its license, or upstream copies kept in the
# repository (paths relative to its root).
CRATE_FILES: dict[str, tuple[str, ...]] = {
    # The license and copyright notices are kept in this file.
    "r-efi-6.0.0": ("AUTHORS",),
    # RoomKit's Rust core, taken from its repository: the license is at the
    # root of the workspace its crates are members of.
    "roomkit-0.3.0": ("../../LICENSE",),
    "roomkit-ai-0.3.0": ("../../LICENSE",),
    "roomkit-core-0.3.0": ("../../LICENSE",),
}
# Published without their license files; copied from the upstream repository
# at the release tag each crate is built from (jni-rs v0.22.4, jni-sys
# v0.4.1-macros, rustls-platform-verifier v/0.7.1).
UPSTREAM = "bindings/python/licenses/upstream"
UPSTREAM_FILES: dict[str, tuple[str, ...]] = {
    "jni-0.22.4": (f"{UPSTREAM}/jni-0.22.4-APACHE.txt", f"{UPSTREAM}/jni-0.22.4-MIT.txt"),
    "jni-macros-0.22.4": (f"{UPSTREAM}/jni-0.22.4-APACHE.txt", f"{UPSTREAM}/jni-0.22.4-MIT.txt"),
    "jni-sys-macros-0.4.1": (
        f"{UPSTREAM}/jni-sys-macros-0.4.1-APACHE.txt",
        f"{UPSTREAM}/jni-sys-macros-0.4.1-MIT.txt",
    ),
    "rustls-platform-verifier-android-0.2.0": (
        f"{UPSTREAM}/rustls-platform-verifier-android-0.2.0-APACHE.txt",
        f"{UPSTREAM}/rustls-platform-verifier-android-0.2.0-MIT.txt",
    ),
}


def cargo_metadata() -> dict[str, Any]:
    return json.loads(
        subprocess.check_output(
            [
                "cargo",
                "metadata",
                "--manifest-path",
                str(ROOT / MANIFEST),
                "--format-version",
                "1",
                "--locked",
            ],
            text=True,
        )
    )


def key(package: dict[str, Any]) -> str:
    return f"{package['name']}-{package['version']}"


def linked(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    """The packages the extension links: those reachable from the root by
    normal dependencies, the root and the repository's own crates among
    them."""
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    reached: set[str] = set()
    pending = [metadata["resolve"]["root"]]
    while pending:
        current = pending.pop()
        if current in reached:
            continue
        reached.add(current)
        pending.extend(
            dependency["pkg"]
            for dependency in nodes[current]["deps"]
            if any(kind["kind"] is None for kind in dependency["dep_kinds"])
        )
    return [packages[package_id] for package_id in sorted(reached)]


def normal_dependencies(metadata: dict[str, Any]) -> list[str]:
    """Locked non-path packages reachable from the root by normal dependencies."""
    return sorted(key(package) for package in linked(metadata) if package["source"] is not None)


def taken_under(expression: str) -> set[str]:
    """The licenses a crate of this `expression` is taken under: of each
    choice it offers, MIT where it is one, else the first."""
    taken: set[str] = set()
    for term in expression.replace("/", " OR ").split(" AND "):
        choices = [choice.strip("() ") for choice in term.split(" OR ")]
        taken.add("MIT" if "MIT" in choices else choices[0])
    return taken


def wheel_license(metadata: dict[str, Any]) -> str:
    """The license expression that is true of the wheel: every license a
    linked crate is taken under, the project's own first."""
    taken: set[str] = set()
    for package in linked(metadata):
        taken |= taken_under(package["license"])
    own = [name for name in OWN_LICENSES if name in taken]
    return " AND ".join(own + sorted(taken - set(own)))


def configured(files: dict[str, tuple[str, ...]], package: dict[str, Any]) -> tuple[str, ...]:
    for pattern, paths in files.items():
        if fnmatch.fnmatchcase(key(package), pattern):
            return paths
    return ()


def notice_files(package: dict[str, Any]) -> list[tuple[str, Path]]:
    """Notice files of one package as (name shown in its heading, path).

    The package's own notices come first, then those of nested components.
    """
    source = Path(package["manifest_path"]).parent
    found = sorted(
        path
        for path in source.rglob("*")
        if path.is_file() and path.name.lower().startswith(NOTICE_PREFIXES)
    )
    own = [path for path in found if path.parent == source]
    if not own:
        own = [source / name for name in configured(CRATE_FILES, package)]
        own += [ROOT / name for name in configured(UPSTREAM_FILES, package)]
    if not own:
        raise RuntimeError(f"Inspect missing notices for {package['name']} {package['version']}")
    nested = [path for path in found if path.parent != source]
    return [
        (path.relative_to(source).as_posix() if path.is_relative_to(source) else path.name, path)
        for path in own + nested
    ]


def render(metadata: dict[str, Any]) -> str:
    parts = [TITLE]
    packages = sorted(metadata["packages"], key=lambda item: (item["name"], item["version"]))
    for package in packages:
        if package["source"] is None:
            continue
        for name, path in notice_files(package):
            parts.append(
                f"\n--- {key(package)}/{name} ({package['license']}) ---\n\n"
                + path.read_text(encoding="utf-8")
            )
    text = "".join(parts)
    return "\n".join(line.rstrip() for line in text.splitlines()) + "\n"


def uncovered(notices: str) -> list[str]:
    """Locked packages of the shipped dependency graph that have no notice."""
    return [
        package
        for package in normal_dependencies(cargo_metadata())
        if f"\n--- {package}/" not in notices
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("action", choices=["write", "check"])
    parser.add_argument("notices", nargs="?", type=Path, help="Notices file to check")
    options = parser.parse_args()
    if options.action == "write":
        (ROOT / OUTPUT).write_text(render(cargo_metadata()), encoding="utf-8")
        return
    notices = options.notices or ROOT / OUTPUT
    missing = uncovered(notices.read_text(encoding="utf-8"))
    if missing:
        sys.exit(f"{notices}: no notice for {', '.join(missing)}")
    print(f"{notices}: notices cover every locked normal dependency")
    stated = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]["license"]
    true = wheel_license(cargo_metadata())
    if stated != true:
        sys.exit(f"pyproject.toml: the wheel's license is {true!r}, not {stated!r}")
    print(f"pyproject.toml: the license is that of what the wheel links: {true}")


if __name__ == "__main__":
    main()
