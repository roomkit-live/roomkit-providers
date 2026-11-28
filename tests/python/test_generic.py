"""The criterion the package is built to: adding a provider changes nothing
of what every provider shares.

What is shared holds no vendor's name, and reads no class's name to tell one
provider from another: the package's own modules and those of each kind
that are not a vendor's, the binding's modules outside its vendors, and the
tests of the shared part. A vendor is known by its module, in the package
and in the binding, which hold the same ones. A vendor's module states what
is its own and enters the native module nowhere: every entry is the shared
part's, with what guards it.

What these tests hold: no shared file names a vendor, however the name is
written, nor reads the name of a class; a vendor's module of the package is
imports and class statements of the documented shape, a configuration's
members with their rules and a provider with nothing of its own, so that it
overrides nothing shared and changes nothing shared as it is imported:

- it imports names from a short list, and no module whole: the two shared
  classes, names of `typing` and of pydantic, RoomKit's configuration of
  the same provider;
- its classes derive from the two shared classes alone, with the keywords
  those take;
- no member and no rule has a name the shared class defines;
- a vendor's module of the binding reads no environment.

What they do not hold, because no search finds it:

- a branch of the shared part on what a provider says of itself when it
  runs (the start of its name, the length of its label), which names no
  vendor and reads no class's name;
- what a member's rule does when it runs with what Python gives every
  function and with what the names it may import lead to;
- what a vendor's module of the binding does beside building its provider.

Those are for the review of a change, and for each provider's own tests,
which run on the same shared code.
"""

from __future__ import annotations

import ast
import re
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
    "python/roomkit_providers/image/__init__.py",
    "python/roomkit_providers/image/_config.py",
    "python/roomkit_providers/image/_provider.py",
    "bindings/python/src/dynamic.rs",
    "bindings/python/src/errors.rs",
    "bindings/python/src/fork.rs",
    "bindings/python/src/image.rs",
    "bindings/python/src/image_vendor.rs",
    "bindings/python/src/lib.rs",
    "bindings/python/src/provider.rs",
    "bindings/python/src/runtime.rs",
    "bindings/python/src/stream.rs",
    "bindings/python/src/testing.rs",
    "bindings/python/src/testing_image.rs",
    "bindings/python/src/vendor.rs",
}
# The tests of the shared part, which run with no vendor.
SHARED_TESTS = {
    "tests/python/test_generic.py",
    "tests/python/test_provider.py",
    "tests/python/test_image_provider.py",
}
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

# The two classes a vendor's own derive from, each by the shared module that
# defines it, and the keywords each takes in a class statement.
SHARED_CLASSES = {"Config": "_config", "Provider": "_provider", "ManagedServer": "_provider"}
OF_A_CONFIGURATION = {"vendor", "stands_for", "decided", "rewritten", "urls", "key"}
OF_A_PROVIDER = {"config"}
# Where else a vendor's module of the package imports names from: for its
# members' types, and RoomKit's configuration of the same provider, a name
# that ends as one.
FOR_THE_MEMBERS = {"__future__", "typing", "pydantic"}
ROOMKIT = "roomkit"
A_CONFIGURATION = "Config"
# How Rust reads the environment, at run time or as it compiles.
ENVIRONMENT = re.compile(r"\b(?:option_)?env\b")


def kinds() -> list[Path]:
    """The package's directories of providers, one per kind."""
    return sorted(path for path in PACKAGE.iterdir() if (path / "__init__.py").is_file())


# The kind of provider whose vendors the binding names by the vendor alone:
# a vendor of another kind is ``<vendor>_<kind>`` there.
LANGUAGE_MODELS = "llm"


def package_modules() -> dict[str, Path]:
    """The vendors' modules of the package, by vendor as the binding names
    it: the public modules of each kind."""
    modules = (module for kind in kinds() for module in kind.glob("*.py"))
    return {
        module.stem
        if module.parent.name == LANGUAGE_MODELS
        else f"{module.stem}_{module.parent.name}": module
        for module in modules
        if not module.name.startswith("_")
    }


def binding_modules() -> dict[str, Path]:
    """The vendors' modules of the binding, by vendor: those beside their
    list."""
    modules = (BINDING / "vendors").glob("*.rs")
    return {module.stem: module for module in modules if module.stem != "mod"}


def vendors() -> set[str]:
    """Every vendor's name as the shared part must not hold it: as the
    binding names it, and as its module of the package does."""
    stems = {module.stem for module in package_modules().values()}
    return set(package_modules()) | set(binding_modules()) | stems


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


# The words of RoomKit's contract and of Python that hold a vendor's name:
# read as the words they are, never as the name.
WORDS_HOLDING_A_NAME = ("metadata", "metaclass")


def unnamed(text: str) -> str:
    """`text` as `letters` reads it, without the words that hold a vendor's
    name."""
    read = letters(text)
    for word in WORDS_HOLDING_A_NAME:
        read = read.replace(word, " ")
    return read


def named_in(files: list[Path], names: set[str]) -> list[str]:
    """Each of `names` each of `files` holds, as ``file: name``."""
    return [
        f"{path.name}: {name}"
        for path in files
        for name in sorted(names)
        if letters(name) in unnamed(path.read_text(encoding="utf-8"))
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


def _is_docstring(node: ast.AST) -> bool:
    value = getattr(node, "value", None)
    return isinstance(node, ast.Expr) and isinstance(getattr(value, "value", None), str)


def _assigns(node: ast.AST, name: str) -> bool:
    """Whether `node` assigns to the one name `name`, a list of texts."""
    if not (isinstance(node, ast.Assign) and isinstance(node.value, ast.List)):
        return False
    targets = [target.id for target in node.targets if isinstance(target, ast.Name)]
    return targets == [name] and all(isinstance(item, ast.Constant) for item in node.value.elts)


def _is_a_member_s_rule(node: ast.AST) -> bool:
    """Whether `node` is the rule of a configuration's member: a function
    of a private name that pydantic's `field_validator` decorates."""
    if not isinstance(node, ast.FunctionDef) or not node.name.startswith("_"):
        return False
    if node.name.startswith("__"):
        return False
    decorators = [
        getattr(getattr(decorator, "func", decorator), "id", None)
        for decorator in node.decorator_list
    ]
    return sorted(decorators) == ["classmethod", "field_validator"]


def imported_beyond_the_list(tree: ast.AST) -> list[str]:
    """What `tree`, a vendor's module, imports that it may not, wherever
    the import is written. A module imported whole is a handle on all it
    holds, so every import is of names, from the short list."""
    beyond = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            beyond += [f"import {alias.name}" for alias in node.names]
        if not isinstance(node, ast.ImportFrom):
            continue
        module = node.module or ""
        for alias in node.names:
            if node.level:
                allowed = node.level == 1 and SHARED_CLASSES.get(alias.name) == module
            elif module.split(".")[0] == ROOMKIT:
                allowed = alias.name.endswith(A_CONFIGURATION)
            else:
                allowed = module in FOR_THE_MEMBERS and alias.name != "*"
            if not allowed:
                beyond.append(f"from {'.' * node.level}{module} import {alias.name}")
    return beyond


def kind_of(node: ast.ClassDef) -> str | None:
    """What `node`, a class statement of a vendor's module, is by its bases
    and its keywords: a configuration, which derives from the shared one
    alone; a provider, which derives from the shared one alone, over its
    configurations; or neither. A first base from elsewhere would make the
    shared class's methods another's."""
    keywords = {keyword.arg for keyword in node.keywords}
    bases = node.bases
    # A managed provider may add the shared lifecycle mixin, before its provider base.
    managed = len(bases) == 2 and isinstance(bases[0], ast.Name) and bases[0].id == "ManagedServer"
    if managed:
        bases = bases[1:]
    if len(bases) != 1:
        return None
    (base,) = bases
    if not managed and isinstance(base, ast.Name) and base.id == "Config":
        stated = "vendor" in keywords and keywords <= OF_A_CONFIGURATION
        return "configuration" if stated else None
    over = base.value if isinstance(base, ast.Subscript) else None
    if isinstance(over, ast.Name) and over.id == "Provider":
        return "provider" if keywords == OF_A_PROVIDER else None
    return None


def defined_in(node: ast.ClassDef) -> set[str]:
    """The names the body of `node` defines: its methods, and what it
    assigns or annotates."""
    names = set()
    for part in node.body:
        if isinstance(part, ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef):
            names.add(part.name)
        elif isinstance(part, ast.AnnAssign | ast.Assign):
            targets = [part.target] if isinstance(part, ast.AnnAssign) else part.targets
            names |= {target.id for target in targets if isinstance(target, ast.Name)}
    return names


def shared_class(kind: Path, module: str, name: str) -> ast.ClassDef | None:
    """The class `name` the shared module `module` of `kind` defines, or
    takes from another kind's, where it takes it; None where it holds no
    such class."""
    tree = ast.parse((kind / f"{module}.py").read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == name:
            return node
        taken = isinstance(node, ast.ImportFrom) and node.level == 2 and node.module
        if taken and name in {alias.name for alias in node.names}:
            other, _, taken_from = node.module.partition(".")
            return shared_class(PACKAGE / other, taken_from, name)
    return None


def defined_by_the_shared_classes(kind: Path) -> dict[str, set[str]]:
    """What each shared class of `kind` defines, by the class's name: the
    classes its shared modules hold."""
    defined = {}
    for name, module in SHARED_CLASSES.items():
        shared = shared_class(kind, module, name)
        if shared is not None:
            defined[name] = defined_in(shared)
    return defined


def taken_from_the_shared_class(tree: ast.Module, defined: dict[str, set[str]]) -> list[str]:
    """The names a class of `tree`, a vendor's module, defines that the
    shared class it derives from defines too: each would replace what is
    written once."""
    taken = []
    for node in tree.body:
        if not isinstance(node, ast.ClassDef):
            continue
        for base in node.bases:
            over = base.value if isinstance(base, ast.Subscript) else base
            shared = defined.get(getattr(over, "id", ""), set())
            taken += [f"{node.name}.{name}" for name in sorted(defined_in(node) & shared)]
    return taken


def reading_the_environment(text: str) -> list[str]:
    """The lines of `text`, a Rust module, that read the environment."""
    return [line.strip() for line in text.splitlines() if ENVIRONMENT.search(line)]


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
        shared = {
            f"roomkit_providers.{kind.name}.{module}"
            for kind in kinds()
            for module in ("_provider", "_config")
        }
        own = {
            f"roomkit_providers.{module.parent.name}.{module.stem}"
            for module in package_modules().values()
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
        for vendor, module in sorted(package_modules().items()):
            own = [module, binding_modules()[vendor], ROOT / VENDOR_LIST]
            self.assertEqual(len(named_in(own, {vendor})), 3, vendor)
        # Each name a vendor goes by, its module's own among them: an image
        # vendor named in the shared part as its module is named is found.
        for vendor in sorted(vendors()):
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
                    # A vendor's name may hold another vendor's name:
                    # the vendor's own is found.
                    self.assertIn(f"sample.py: {vendor}", named_in([sample], vendors()), line)
        with tempfile.TemporaryDirectory() as directory:
            sample = Path(directory) / "sample.py"
            sample.write_text("if label == self.label:\n", encoding="utf-8")
            self.assertEqual(named_in([sample], vendors()), [])
        # A word of the contract that holds a vendor's name is no name; the
        # name beside it still is.
        held = {
            vendor
            for vendor in vendors()
            if any(letters(vendor) in word for word in WORDS_HOLDING_A_NAME)
        }
        for vendor in sorted(held):
            with tempfile.TemporaryDirectory() as directory:
                sample = Path(directory) / "sample.py"
                sample.write_text("context.metadata[API_KEY_METADATA_KEY]\n", encoding="utf-8")
                self.assertEqual(named_in([sample], {vendor}), [], vendor)
                sample.write_text(f'context.metadata["{vendor}"]\n', encoding="utf-8")
                self.assertEqual(named_in([sample], {vendor}), [f"sample.py: {vendor}"])

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

    def test_a_vendor_s_module_is_imports_and_class_statements(self) -> None:
        """Nothing runs when a vendor's module is imported but its imports
        and its class statements: it changes nothing of the shared part.
        Of that part it imports the two classes its own derive from, and
        beyond it names from a short list: no module whole, which would be
        a handle on the shared part, and nothing that reads the process."""
        for vendor, module in sorted(package_modules().items()):
            with self.subTest(vendor=vendor):
                tree = ast.parse(module.read_text(encoding="utf-8"))
                others = [
                    type(node).__name__
                    for node in tree.body
                    if not (
                        isinstance(node, ast.Import | ast.ImportFrom | ast.ClassDef)
                        or _is_docstring(node)
                        or _assigns(node, "__all__")
                    )
                ]
                self.assertEqual(others, [])
                shared = {
                    f"{node.module}.{alias.name}"
                    for node in tree.body
                    if isinstance(node, ast.ImportFrom) and node.level
                    for alias in node.names
                }
                expected = {"_config.Config", "_provider.Provider"}
                if any(
                    isinstance(node, ast.ClassDef)
                    and any(
                        isinstance(base, ast.Name) and base.id == "ManagedServer"
                        for base in node.bases
                    )
                    for node in tree.body
                ):
                    expected.add("_provider.ManagedServer")
                self.assertEqual(shared, expected)
                self.assertEqual(imported_beyond_the_list(tree), [])

    def test_a_vendor_s_classes_hold_a_class_statement_s_worth(self) -> None:
        """A vendor's provider is a class statement and a docstring: it
        defines no method, so it overrides none. Its configuration states
        its members, and the rules of a member that RoomKit's configuration
        has: nothing else."""
        for vendor, module in sorted(package_modules().items()):
            tree = ast.parse(module.read_text(encoding="utf-8"))
            classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
            kinds_held = sorted(str(kind_of(node)) for node in classes)
            self.assertEqual(kinds_held, ["configuration", "provider"], vendor)
            for node in classes:
                with self.subTest(vendor=vendor, name=node.name):
                    body = [part for part in node.body if not _is_docstring(part)]
                    if kind_of(node) == "provider":
                        self.assertEqual([type(part).__name__ for part in body], [])
                    else:
                        others = [
                            getattr(part, "name", type(part).__name__)
                            for part in body
                            if not (isinstance(part, ast.AnnAssign) or _is_a_member_s_rule(part))
                        ]
                        self.assertEqual(others, [])

    def test_a_vendor_s_class_takes_no_name_of_the_shared_class(self) -> None:
        """A member or a rule named as something the shared class defines
        would replace it: what decides a switch, what hides a value."""
        for vendor, module in sorted(package_modules().items()):
            with self.subTest(vendor=vendor):
                defined = defined_by_the_shared_classes(module.parent)
                for base in ("Config", "Provider"):
                    self.assertGreater(len(defined[base]), 5)
                if module.parent.name == LANGUAGE_MODELS:
                    self.assertGreaterEqual(defined["ManagedServer"], {"base_url", "start"})
                tree = ast.parse(module.read_text(encoding="utf-8"))
                self.assertEqual(taken_from_the_shared_class(tree, defined), [])

    def test_a_vendor_s_module_of_the_binding_reads_no_environment(self) -> None:
        """A provider is built from its configuration and from the key it
        is given: a key or an endpoint read from the environment would be a
        configuration nobody stated."""
        for module in sorted((BINDING / "vendors").rglob("*.rs")):
            with self.subTest(module=module.name):
                text = module.read_text(encoding="utf-8")
                self.assertEqual(reading_the_environment(text), [])

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
                self.assertRegex(text, r"impl (Image)?Vendor for")
                # Nothing Python calls is declared there.
                self.assertNotIn("pyo3", text)
                self.assertNotIn("catch_unwind", text)


# A vendor's module of the package in the documented shape, and the lines of
# one of the binding: what each check is silent on.
SAMPLE = '''"""A sample."""

from __future__ import annotations

from typing import Any

from pydantic import SecretStr, field_validator
from roomkit.providers.sample import SampleConfig as RoomKitSampleConfig

from ._config import Config
from ._provider import Provider

__all__ = ["Sample", "SampleConfig"]


class SampleConfig(Config, vendor="sample", stands_for=RoomKitSampleConfig, decided=()):
    """A configuration."""

    api_key: SecretStr
    base_url: str | None = None
    extra_body: dict[str, Any] | None = None

    @field_validator("base_url")
    @classmethod
    def _as_given(cls, value: str | None) -> str | None:
        return value


class Sample(Provider[SampleConfig | RoomKitSampleConfig], config=SampleConfig):
    """A provider."""
'''
SAMPLE_OF_THE_BINDING = """
    // The environment's proxies are the client's to read.
    fn keyed(config: Self::Config, key: Option<String>) -> Result<Self::Provider, Failure> {
        Sample::new(config, key)
    }
"""
DEFINED = {"Config": {"_decide", "model_config", "model_post_init"}, "Provider": {"generate"}}
PROVIDER = "class Sample(Provider[SampleConfig | RoomKitSampleConfig], config=SampleConfig):"
CONFIGURATION = "class SampleConfig(Config, vendor="
RULE = "    def _as_given(cls, value: str | None) -> str | None:\n"


class ChecksBiteTests(unittest.TestCase):
    """Each check on a sample: silent on the documented shape, and finding
    each way out of it that a vendor's module was seen to take."""

    def written(self, old: str, new: str) -> ast.Module:
        """The sample with `new` where it holds `old`."""
        self.assertEqual(SAMPLE.count(old), 1, old)
        return ast.parse(SAMPLE.replace(old, new))

    def test_the_documented_shape_passes_each_check(self) -> None:
        tree = ast.parse(SAMPLE)
        classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
        self.assertEqual(imported_beyond_the_list(tree), [])
        self.assertEqual([kind_of(node) for node in classes], ["configuration", "provider"])
        self.assertEqual(taken_from_the_shared_class(tree, DEFINED), [])
        self.assertEqual(reading_the_environment(SAMPLE_OF_THE_BINDING), [])

    def test_an_import_beyond_the_list_is_found(self) -> None:
        shared = "from ._config import Config\n"
        at_the_top = [
            # The shared module whole, or a name of it that is no class of
            # the two: a handle to change the shared part through.
            "import package.kind._provider",
            "from . import _provider",
            "from package.kind import _provider",
            "from ._provider import _request",
            # What reads the process.
            "import os",
            "from os import environ",
            # A name of RoomKit that is no configuration.
            "from roomkit.providers.sample import SampleProvider",
            "from pydantic import *",
        ]
        for found in at_the_top:
            with self.subTest(found=found):
                tree = self.written(shared, f"{found}\n{shared}")
                self.assertEqual(imported_beyond_the_list(tree), [found])
        for found in ["import os", "from os import environ"]:
            with self.subTest(found=found, inside="a rule"):
                tree = self.written(RULE, f"{RULE}        {found}\n")
                self.assertEqual(imported_beyond_the_list(tree), [found])

    def test_a_class_of_another_shape_is_found(self) -> None:
        others = {
            # A first base from elsewhere: `generate` would be that class's.
            PROVIDER: PROVIDER.replace("(Provider[", "(RoomKitSampleProvider, Provider["),
            CONFIGURATION: "class SampleConfig(Config, RoomKitSampleConfig, vendor=",
            "config=SampleConfig):": "config=SampleConfig, metaclass=Other):",
            "decided=()": "decided=(), **more",
            "(Provider[SampleConfig | RoomKitSampleConfig], ": "(Provider, ",
            "class Sample(Provider[": "class Sample(Other[",
            "class SampleConfig(Config, ": "class SampleConfig(Other, ",
            'vendor="sample", ': "",
        }
        for old, new in others.items():
            with self.subTest(new=new):
                tree = self.written(old, new)
                classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
                self.assertIn(None, [kind_of(node) for node in classes])

    def test_a_name_of_the_shared_class_is_found(self) -> None:
        taken = {
            # A member's rule of the allowed shape, named as what decides
            # the switches.
            "SampleConfig._decide": ("def _as_given(", "def _decide("),
            "SampleConfig.model_config": (
                "    api_key: SecretStr\n",
                "    api_key: SecretStr\n    model_config: dict[str, Any] = {}\n",
            ),
            "SampleConfig.model_post_init": ("def _as_given(", "def model_post_init("),
            "Sample.generate": (
                '    """A provider."""\n',
                '    """A provider."""\n\n    generate = None\n',
            ),
        }
        for found, (old, new) in taken.items():
            with self.subTest(found=found):
                tree = self.written(old, new)
                self.assertEqual(taken_from_the_shared_class(tree, DEFINED), [found])

    def test_a_read_of_the_environment_is_found(self) -> None:
        built = "Sample::new(config, key)"
        reads = [
            'Sample::new(config, key.or_else(|| std::env::var("SAMPLE_API_KEY").ok()))',
            'Sample::new(config, key.or_else(|| env::var("SAMPLE_API_KEY").ok()))',
            'Sample::new(config, std::env::var_os("SAMPLE_API_KEY").map(read))',
            'Sample::new(config, option_env!("SAMPLE_API_KEY").map(str::to_owned))',
            'Sample::new(config, Some(env!("SAMPLE_API_KEY").to_owned()))',
        ]
        for read in reads:
            with self.subTest(read=read):
                written = SAMPLE_OF_THE_BINDING.replace(built, read)
                self.assertEqual(reading_the_environment(written), [read])
        self.assertEqual(reading_the_environment("use std::env;"), ["use std::env;"])


if __name__ == "__main__":
    unittest.main()
