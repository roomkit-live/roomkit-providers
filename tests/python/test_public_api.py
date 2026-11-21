"""The installed package's public API against its committed snapshot.

Any change to the public Python surface, a compatible addition included,
fails here until ``make api-snapshot`` rewrites the snapshot in the same
commit.
"""

import ast
import inspect
import json
import shutil
import tempfile
import unittest
from pathlib import Path

import api_signatures
from api_signatures import Package
from roomkit_providers import _native

HERE = Path(__file__).resolve().parent
PACKAGE = Package("roomkit_providers")


class SnapshotTests(unittest.TestCase):
    def test_the_snapshot_holds_the_whole_public_surface(self):
        drift = api_signatures.drift(HERE, PACKAGE)
        self.assertEqual(drift, [], "run make api-snapshot and commit the rewritten files")

    def test_a_snapshot_that_differs_from_the_code_is_drift(self):
        """A name, a parameter or a key the snapshot lacks, or holds and the
        code does not, is drift."""
        provider = "roomkit_providers.llm.openai.OpenAI"
        config = "roomkit_providers.llm.openai.OpenAIConfig"
        changes = [
            (api_signatures.NAMES, lambda names: names["roomkit_providers.llm.openai"].pop()),
            (api_signatures.SIGNATURES, lambda specs: specs[provider]["generate"].append("x=1")),
            (api_signatures.SIGNATURES, lambda specs: specs[provider].pop("close")),
            (api_signatures.SIGNATURES, lambda specs: specs[provider]["__bases__"].pop()),
            (api_signatures.SIGNATURES, lambda specs: specs[config]["__init__"].remove("model")),
            (
                api_signatures.SIGNATURES,
                lambda specs: specs[config]["__init__"].append("region=None"),
            ),
        ]
        for name, change in changes:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                for held in (api_signatures.NAMES, api_signatures.SIGNATURES):
                    shutil.copy(HERE / held, directory / held)
                path = directory / name
                snapshot = json.loads(path.read_text(encoding="utf-8"))
                change(snapshot)
                path.write_text(json.dumps(snapshot), encoding="utf-8")
                self.assertEqual(len(api_signatures.drift(directory, PACKAGE)), 1)


class NativeStubTests(unittest.TestCase):
    """The stub of the native module, held to the module: what a module
    that is distributed holds, each class with its members, and no more."""

    # What a module built for the tests holds beside, declared nowhere.
    FOR_TESTS = frozenset(
        {"_scripted", "_Scripted", "_released", "_handing_panics", "_handing_ends"}
    )

    @staticmethod
    def declared() -> dict[str, set[str]]:
        """Each name the stub declares, with the members of a class."""
        stub = Path(_native.__file__).with_name("_native.pyi")
        tree = ast.parse(stub.read_text(encoding="utf-8"))
        defined = (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)
        return {
            node.name: {
                member.name for member in getattr(node, "body", []) if isinstance(member, defined)
            }
            if isinstance(node, ast.ClassDef)
            else set()
            for node in tree.body
            if isinstance(node, defined)
        }

    def test_the_stub_declares_what_the_module_holds_and_no_more(self):
        declared = self.declared()
        held = {name for name in vars(_native) if not name.startswith("__")}
        for_tests = self.FOR_TESTS if "_scripted" in held else frozenset()
        self.assertEqual(held - for_tests, set(declared))
        self.assertEqual(held & self.FOR_TESTS, for_tests)

    def test_each_class_of_the_stub_has_the_members_the_module_gives_it(self):
        for name, members in self.declared().items():
            held = getattr(_native, name)
            if not isinstance(held, type):
                continue
            with self.subTest(name=name):
                given = {member for member in vars(held) if not member.startswith("__")}
                given |= {"__call__"} & set(vars(held))
                self.assertEqual(given, members)


class DescribeTests(unittest.TestCase):
    """How the snapshot writes a class, on classes of a fake package."""

    package = Package(__name__)

    def test_a_class_with_its_properties_and_its_calls(self):
        class Provider:
            def __init__(self, config: object) -> None: ...

            @classmethod
            def available_models(cls) -> list: ...

            @property
            def model_name(self) -> str: ...

            async def generate(self, request: dict) -> dict: ...

            async def generate_stream(self, request: dict):
                yield ""

        Provider.__module__ = __name__
        self.assertEqual(
            api_signatures.describe(Provider, self.package, {}),
            {
                "__init__": ["def", "config"],
                "available_models": ["classmethod"],
                "generate": ["async def", "request"],
                "generate_stream": ["async gen def", "request"],
                "model_name": ["property"],
            },
        )

    def test_a_typed_dict_keeps_its_keys_required_or_not(self):
        from typing import NotRequired, TypedDict

        class Part(TypedDict):
            type: str
            mime_type: NotRequired[str | None]

        Part.__module__ = __name__
        self.assertEqual(
            api_signatures.describe(Part, self.package, {}),
            {"type": ["key", "required"], "mime_type": ["key", "optional"]},
        )

    def test_a_public_base_of_the_package_is_written(self):
        class Base(Exception):
            pass

        class Child(Base):
            pass

        Base.__module__ = Child.__module__ = __name__
        self.assertEqual(
            api_signatures.describe(Child, self.package, {id(Base): "pkg.Base"}),
            {"__bases__": ["bases", "pkg.Base"]},
        )

    def test_a_base_of_another_package_is_written_where_it_is_defined(self):
        class Foreign:
            pass

        class Child(Foreign, Exception):
            pass

        Foreign.__module__ = "another_package.models"
        Foreign.__qualname__ = "Foreign"
        Child.__module__ = __name__
        self.assertEqual(
            api_signatures.describe(Child, self.package, {}),
            {"__bases__": ["bases", "another_package.models.Foreign"]},
        )

    def test_a_base_the_package_does_not_export_gives_way_to_its_own(self):
        class Foreign:
            pass

        class Hidden(Foreign):
            def close(self) -> None: ...

        class Child(Hidden):
            pass

        Foreign.__module__ = "another_package.models"
        Foreign.__qualname__ = "Foreign"
        Hidden.__module__ = Child.__module__ = __name__
        self.assertEqual(
            api_signatures.describe(Child, self.package, {}),
            {"__bases__": ["bases", "another_package.models.Foreign"], "close": ["def"]},
        )

    def test_a_model_built_from_its_fields_is_written_with_them(self):
        class Config:
            __signature__ = inspect.Signature(
                [
                    inspect.Parameter("model", inspect.Parameter.KEYWORD_ONLY),
                    inspect.Parameter("timeout", inspect.Parameter.KEYWORD_ONLY, default=30.0),
                ]
            )

        Config.__module__ = __name__
        self.assertEqual(
            api_signatures.describe(Config, self.package, {}),
            {"__init__": ["def", "*", "model", "timeout=30.0"]},
        )


if __name__ == "__main__":
    unittest.main()
