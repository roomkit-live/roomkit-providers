"""The criterion the package is built to: adding a provider changes nothing
of what every provider shares.

What is shared holds no vendor's name, and reads no class's name to tell one
provider from another: the package's own modules and those of each kind
that are not a vendor's, the binding's modules outside its vendors, and the
tests of the shared part. A vendor is known by its module, in the package
and in the binding, which hold the same ones. A vendor's module states what
is its own and enters the native module nowhere: every entry is the shared
part's, with what guards it.
"""

from __future__ import annotations

import ast
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "python/roomkit_providers"
BINDING = ROOT / "bindings/python/src"
TESTS = Path(__file__).resolve().parent

# What every provider shares, file by file. A file joins the list when the
# shared part gains one: a provider adds none, and changes none.
SHARED_MODULES = {
    "python/roomkit_providers/__init__.py",
    "python/roomkit_providers/_native.pyi",
    "python/roomkit_providers/_process.py",
    "python/roomkit_providers/llm/__init__.py",
    "python/roomkit_providers/llm/_config.py",
    "python/roomkit_providers/llm/_failure.py",
    "python/roomkit_providers/llm/_provider.py",
    "bindings/python/src/errors.rs",
    "bindings/python/src/fork.rs",
    "bindings/python/src/lib.rs",
    "bindings/python/src/provider.rs",
    "bindings/python/src/runtime.rs",
    "bindings/python/src/stream.rs",
    "bindings/python/src/testing.rs",
    "bindings/python/src/vendor.rs",
}
# The tests of the shared part, which run with no vendor.
SHARED_TESTS = {"tests/python/test_generic.py", "tests/python/test_provider.py"}
# The one shared file a provider adds a line to: the list of the vendors.
VENDOR_LIST = "bindings/python/src/vendors/mod.rs"

# What a vendor's module of the package does not define: each is written
# once, for every provider.
WRITTEN_ONCE = {
    "__init__",
    "__new__",
    "__init_subclass__",
    "__pydantic_init_subclass__",
    "__repr__",
    "__repr_args__",
    "__str__",
    "model_post_init",
}
# How a class's name is read: no shared module reads one.
NAMES_OF_A_CLASS = {"__name__", "__qualname__", "__module__", "__class__"}


def kinds() -> list[Path]:
    """The package's directories of providers, one per kind."""
    return sorted(path for path in PACKAGE.iterdir() if (path / "__init__.py").is_file())


def package_modules() -> dict[str, Path]:
    """The vendors' modules of the package, by vendor: the public modules of
    each kind."""
    modules = (module for kind in kinds() for module in kind.glob("*.py"))
    return {module.stem: module for module in modules if not module.name.startswith("_")}


def binding_modules() -> dict[str, Path]:
    """The vendors' modules of the binding, by vendor: those beside their
    list."""
    modules = (BINDING / "vendors").glob("*.rs")
    return {module.stem: module for module in modules if module.stem != "mod"}


def vendors() -> set[str]:
    return set(package_modules()) | set(binding_modules())


def found_shared() -> set[str]:
    """Every file of the package and of the binding that is no vendor's."""
    own = {*package_modules().values(), *binding_modules().values(), ROOT / VENDOR_LIST}
    package = [*PACKAGE.rglob("*.py"), *PACKAGE.rglob("*.pyi")]
    found = [path for path in [*package, *BINDING.rglob("*.rs")] if path not in own]
    return {str(path.relative_to(ROOT)) for path in found}


def letters(text: str) -> str:
    """`text` as its letters and digits alone, in lower case: a name written
    in two pieces, or in another case, reads as the name."""
    return "".join(character for character in text.lower() if character.isalnum())


def named_in(files: list[Path], names: set[str]) -> list[str]:
    """Each of `names` each of `files` holds, as ``file: name``."""
    return [
        f"{path.name}: {name}"
        for path in files
        for name in sorted(names)
        if letters(name) in letters(path.read_text(encoding="utf-8"))
    ]


def imports(path: Path) -> set[str]:
    """The modules `path` imports, a name imported from one included."""
    found = set()
    for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
        if isinstance(node, ast.Import):
            found |= {alias.name for alias in node.names}
        elif isinstance(node, ast.ImportFrom):
            module = "." * node.level + (node.module or "")
            found |= {module, *(f"{module}.{alias.name}" for alias in node.names)}
    return found


class GenericTests(unittest.TestCase):
    def test_the_package_and_the_binding_hold_the_same_vendors(self) -> None:
        self.assertEqual(set(package_modules()), set(binding_modules()))
        self.assertGreaterEqual(len(vendors()), 1)

    def test_what_is_shared_is_every_file_that_is_no_vendor_s(self) -> None:
        self.assertEqual(found_shared(), SHARED_MODULES)
        for test in SHARED_TESTS:
            self.assertTrue((ROOT / test).is_file(), test)

    def test_a_test_of_the_shared_part_is_held_to_the_criterion(self) -> None:
        """A test that imports what every provider shares, and no vendor's
        module, is a test of the shared part."""
        shared = {"roomkit_providers.llm._provider", "roomkit_providers.llm._config"}
        own = {
            f"roomkit_providers.{kind.name}.{vendor}" for kind in kinds() for vendor in vendors()
        }
        of_the_shared_part = {
            f"tests/python/{test.name}"
            for test in TESTS.glob("*.py")
            if imports(test) & shared and not imports(test) & own
        }
        self.assertEqual(of_the_shared_part | {"tests/python/test_generic.py"}, SHARED_TESTS)

    def test_what_every_provider_shares_holds_no_vendor_s_name(self) -> None:
        files = [ROOT / name for name in sorted(SHARED_MODULES | SHARED_TESTS)]
        self.assertEqual(named_in(files, vendors()), [], "a provider's own part is its module's")

    def test_the_search_finds_a_vendor_s_name_however_it_is_written(self) -> None:
        """The search bites: on each vendor's own modules, on the list of
        the vendors, and on a name written in pieces or in another case."""
        for vendor in sorted(vendors()):
            own = [package_modules()[vendor], binding_modules()[vendor], ROOT / VENDOR_LIST]
            self.assertEqual(len(named_in(own, {vendor})), 3, vendor)
            half = len(vendor) // 2
            written = [
                f'if label == "{vendor}":',
                f'if label == "{vendor[:half]}" + "{vendor[half:]}":',
                f'concat!("{vendor[:half]}", "{vendor[half:]}")',
                f"class {vendor.upper()}Config:",
                f"type(self).__name__ == {vendor.title()!r}",
            ]
            for line in written:
                with tempfile.TemporaryDirectory() as directory:
                    sample = Path(directory) / "sample.py"
                    sample.write_text(f"{line}\n", encoding="utf-8")
                    self.assertEqual(len(named_in([sample], vendors())), 1, line)
        with tempfile.TemporaryDirectory() as directory:
            sample = Path(directory) / "sample.py"
            sample.write_text("if label == self.label:\n", encoding="utf-8")
            self.assertEqual(named_in([sample], vendors()), [])

    def test_nothing_shared_reads_the_name_of_a_class(self) -> None:
        """A provider is told from another by nothing: no shared module of
        the package reads a class's name, and none of the binding a type's."""
        read = []
        for name in sorted(SHARED_MODULES):
            path = ROOT / name
            if path.suffix == ".rs":
                read += [f"{name}: type_name"] * ("type_name" in path.read_text(encoding="utf-8"))
                continue
            tree = ast.parse(path.read_text(encoding="utf-8"))
            read += [
                f"{name}: {node.attr}"
                for node in ast.walk(tree)
                if isinstance(node, ast.Attribute) and node.attr in NAMES_OF_A_CLASS
            ]
        self.assertEqual(read, [])

    def test_a_vendor_s_module_enters_the_native_module_nowhere(self) -> None:
        """What guards an entry into the native module is written once: a
        vendor's module has no entry of its own to leave it out of."""
        for vendor, module in sorted(package_modules().items()):
            with self.subTest(module=module.name):
                reached = {
                    name for name in imports(module) if "_native" in name or "_process" in name
                }
                self.assertEqual(reached, set())
                self.assertNotIn("_native", module.read_text(encoding="utf-8"))
                tree = ast.parse(module.read_text(encoding="utf-8"))
                defined = {
                    node.name
                    for node in ast.walk(tree)
                    if isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef)
                }
                self.assertEqual(defined & WRITTEN_ONCE, set(), vendor)
        for module in sorted(binding_modules().values()):
            with self.subTest(module=module.name):
                text = module.read_text(encoding="utf-8")
                self.assertIn("impl Vendor for", text)
                # Nothing Python calls is declared there.
                self.assertNotIn("pyo3", text)
                self.assertNotIn("catch_unwind", text)


if __name__ == "__main__":
    unittest.main()
