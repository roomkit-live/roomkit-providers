"""The criterion the package is built to: adding a provider changes nothing
of what every provider shares.

What is shared holds no vendor's name: the package's own modules and those
of each kind that are not a vendor's, the binding's modules outside its
vendors, and the tests of the shared part. A vendor is known by its module,
in the package and in the binding, which hold the same ones.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "python/roomkit_providers"
BINDING = ROOT / "bindings/python/src"


def kinds() -> list[Path]:
    """The package's directories of providers, one per kind."""
    return sorted(path for path in PACKAGE.iterdir() if (path / "__init__.py").is_file())


def package_vendors() -> set[str]:
    """The vendors the package holds: the public modules of each kind."""
    modules = (module for kind in kinds() for module in kind.glob("*.py"))
    return {module.stem for module in modules if not module.name.startswith("_")}


def binding_vendors() -> set[str]:
    """The vendors the binding holds: the modules beside their list."""
    return {module.stem for module in (BINDING / "vendors").glob("*.rs")} - {"mod"}


def shared() -> list[Path]:
    """Every file of what all providers share."""
    package = sorted(PACKAGE.glob("_*.py"))
    for kind in kinds():
        package += [kind / "__init__.py", *sorted(kind.glob("_*.py"))]
    return [*package, *sorted(BINDING.glob("*.rs")), Path(__file__).with_name("test_provider.py")]


class GenericTests(unittest.TestCase):
    def test_the_package_and_the_binding_hold_the_same_vendors(self) -> None:
        self.assertEqual(package_vendors(), binding_vendors())
        self.assertIn("openai", package_vendors())

    def test_what_every_provider_shares_holds_no_vendor_s_name(self) -> None:
        files = shared()
        self.assertGreaterEqual(len(files), 10, files)
        named = [
            f"{path.relative_to(ROOT)}: {vendor}"
            for path in files
            for vendor in sorted(package_vendors() | binding_vendors())
            if vendor in path.read_text(encoding="utf-8").lower()
        ]
        self.assertEqual(named, [], "a provider's own part belongs in its own module")

    def test_a_vendor_s_name_in_a_shared_file_is_found(self) -> None:
        """The search bites: the list of vendors, which names each, is
        found to."""
        listing = (BINDING / "vendors/mod.rs").read_text(encoding="utf-8").lower()
        self.assertTrue(all(vendor in listing for vendor in binding_vendors()))


if __name__ == "__main__":
    unittest.main()
