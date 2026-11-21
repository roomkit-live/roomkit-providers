"""The public Python API of the installed package, as a snapshot, and its drift.

Two files hold it, in the snapshot's directory:

- ``public_api_names.json``: every name each public module exports, by
  module. A module's names are its ``__all__``, or, without one, the public
  classes and functions it defines itself. A module is public when no part of
  its dotted name starts with ``_``.
- ``public_api_signatures.json``: everything the package defines and a public
  module exports, keyed by the shortest path it is exported under: a
  function's parameters, a class's constructor, its public methods and
  properties, its public bases from the package; the keys of a typed dict,
  each required or not; the values of a literal type and the members of a
  union. A parameter is written as ``inspect`` writes it, without its
  annotation: ``name``, ``name=default``, ``*args``, ``**kwargs``, and the
  ``/`` and ``*`` markers. A member's first item says what it is: ``def``,
  ``async def``, ``async gen def``, ``classmethod``, ``staticmethod`` (each
  possibly ``async``), ``property``, ``key``, ``bases``, ``literal`` or
  ``union``.

The package's snapshot leaves out its RoomKit adapter, ``roomkit_providers.roomkit``,
which is importable only with RoomKit: the adapter has a snapshot of its own,
in ``tests/python/roomkit``, checked where RoomKit is installed.

``test_public_api.py`` fails when the code and the files differ, a compatible
addition included. The commit that changes the surface rewrites them with
``make api-snapshot``, which runs this module against a freshly built wheel:

    python api_signatures.py write roomkit_providers DIRECTORY
    python api_signatures.py write roomkit_providers.roomkit DIRECTORY

Annotations are left out, a typed dict's keys aside: whether a type widens or
narrows is not read mechanically. What a call does is not here either.
"""

from __future__ import annotations

import importlib
import inspect
import json
import pkgutil
import re
import sys
import types
import typing
from dataclasses import dataclass
from functools import cached_property
from pathlib import Path
from types import ModuleType
from typing import Any

NAMES = "public_api_names.json"
SIGNATURES = "public_api_signatures.json"

_P = inspect.Parameter
_ADDRESS = re.compile(r" at 0x[0-9a-fA-F]+")

Spec = list[str]


# The packages beneath a package that have a snapshot of their own.
APART = {"roomkit_providers": ("roomkit_providers.roomkit",)}


@dataclass(frozen=True)
class Package:
    """An installed package whose public modules are snapshotted: itself and
    what is beneath it, the packages snapshotted apart left out."""

    name: str

    def _apart(self, module: str) -> bool:
        apart = APART.get(self.name, ())
        return any(module == name or module.startswith(f"{name}.") for name in apart)

    def modules(self) -> list[ModuleType]:
        """The package and each public module beneath it, imported."""
        root = importlib.import_module(self.name)
        found = [root]
        prefix = f"{self.name}."
        for info in pkgutil.iter_modules(root.__path__, prefix):
            found += self._beneath(info)
        return sorted(found, key=lambda module: module.__name__)

    def _beneath(self, info: pkgutil.ModuleInfo) -> list[ModuleType]:
        """The public module `info` names and those beneath it, imported."""
        if info.name.rpartition(".")[2].startswith("_") or self._apart(info.name):
            return []
        module = importlib.import_module(info.name)
        found = [module]
        if info.ispkg:
            for inner in pkgutil.iter_modules(module.__path__, f"{info.name}."):
                found += self._beneath(inner)
        return found

    def owns(self, obj: Any) -> bool:
        """Whether ``obj`` is a class or function the package defines."""
        if not (inspect.isclass(obj) or inspect.isfunction(obj)):
            return False
        module = getattr(obj, "__module__", None) or ""
        return module == self.name or module.startswith(f"{self.name}.")


def exported_names(package: Package) -> dict[str, list[str]]:
    """Every name each public module exports, sorted, by module."""
    names: dict[str, list[str]] = {}
    for module in package.modules():
        exported = sorted(_exports(module, package))
        if exported:
            names[module.__name__] = exported
    return names


def build(package: Package) -> dict[str, dict[str, Spec]]:
    """The signatures, keyed by the shortest path each object is exported under."""
    paths: dict[int, str] = {}
    objects: dict[int, Any] = {}
    aliases: dict[str, Any] = {}
    for module in package.modules():
        for name in _exports(module, package):
            obj = getattr(module, name)
            path = f"{module.__name__}.{name}"
            if _is_alias(obj):
                aliases[path] = obj
                continue
            if not package.owns(obj):
                continue
            known = paths.get(id(obj))
            if known is None or (path.count("."), path) < (known.count("."), known):
                paths[id(obj)] = path
                objects[id(obj)] = obj
    described = {paths[key]: describe(objects[key], package, paths) for key in paths}
    described.update({path: _alias_spec(alias, paths) for path, alias in aliases.items()})
    return dict(sorted(described.items()))


def describe(obj: Any, package: Package, paths: dict[int, str]) -> dict[str, Spec]:
    """A class's or a function's members, each as the snapshot writes it."""
    if not inspect.isclass(obj):
        spec = _callable_spec(obj, "def", drop_first=False)
        return {} if spec is None else {"__call__": spec}
    if typing.is_typeddict(obj):
        required = obj.__required_keys__
        return {
            name: ["key", "required" if name in required else "optional"]
            for name in obj.__annotations__
        }
    members: dict[str, Spec] = {}
    bases = [_base(base, paths) for base in obj.__bases__]
    bases = [base for base in bases if base is not None]
    if bases:
        members["__bases__"] = ["bases", *bases]
    if _builds_itself(obj, package):
        try:
            signature = inspect.signature(obj)
        except (TypeError, ValueError):
            signature = None
        if signature is not None:
            members["__init__"] = ["def", *_tokens(signature, drop_first=False)]
    for klass in obj.__mro__:
        if not package.owns(klass):
            continue
        for name, value in vars(klass).items():
            if name.startswith("_") or name in members:
                continue
            spec = _member_spec(value)
            if spec is not None:
                members[name] = spec
    return dict(sorted(members.items()))


def _base(base: type, paths: dict[int, str]) -> str | None:
    """A base class as the snapshot names it: the path the package exports it
    under, or, for a class of another package, where that package defines
    it. A class of the standard library is left out."""
    if id(base) in paths:
        return paths[id(base)]
    top = base.__module__.partition(".")[0]
    if top in sys.stdlib_module_names or top == "builtins":
        return None
    return f"{base.__module__}.{base.__qualname__}"


def drift(directory: Path, package: Package) -> list[str]:
    """Where the code differs from the snapshot in ``directory``, both ways."""
    found: list[str] = []
    held_names = _read(directory / NAMES)
    names = exported_names(package)
    for module in sorted(set(held_names) | set(names)):
        before, after = set(held_names.get(module, ())), set(names.get(module, ()))
        found += [
            f"{module}.{name}: exported, not in the snapshot" for name in sorted(after - before)
        ]
        found += [
            f"{module}.{name}: in the snapshot, no longer exported"
            for name in sorted(before - after)
        ]
    held = _read(directory / SIGNATURES)
    signatures = build(package)
    for path in sorted(set(held) | set(signatures)):
        before, after = held.get(path), signatures.get(path)
        if before is None or after is None:
            found.append(
                f"{path}: " + ("not in the snapshot" if before is None else "no longer exported")
            )
            continue
        for member in sorted(set(before) | set(after)):
            if before.get(member) != after.get(member):
                where = path if member == "__call__" else f"{path}.{member}"
                found.append(
                    f"{where}: {before.get(member)} in the snapshot, "
                    f"{after.get(member)} in the code"
                )
    return found


def write(directory: Path, package: Package) -> None:
    """Rewrite the snapshot in ``directory`` from the installed code."""
    signatures = build(package)
    (directory / NAMES).write_text(
        json.dumps(exported_names(package), indent=2) + "\n", encoding="utf-8"
    )
    blocks = []
    for path, members in signatures.items():
        lines = [
            f"    {json.dumps(name)}: {json.dumps(spec, ensure_ascii=False)}"
            for name, spec in members.items()
        ]
        body = "{\n" + ",\n".join(lines) + "\n  }" if lines else "{}"
        blocks.append(f"  {json.dumps(path)}: {body}")
    (directory / SIGNATURES).write_text("{\n" + ",\n".join(blocks) + "\n}\n", encoding="utf-8")


def _read(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def _exports(module: ModuleType, package: Package) -> list[str]:
    listed = getattr(module, "__all__", None)
    if listed is not None:
        return [name for name in listed if not name.startswith("__")]
    return [
        name
        for name, value in vars(module).items()
        if not name.startswith("_") and package.owns(value) and value.__module__ == module.__name__
    ]


def _is_alias(obj: Any) -> bool:
    """Whether ``obj`` is a literal type or a union: a name for values or for
    other types, which no class stands for."""
    return typing.get_origin(obj) in (typing.Literal, typing.Union, types.UnionType)


def _alias_spec(alias: Any, paths: dict[int, str]) -> dict[str, Spec]:
    if typing.get_origin(alias) is typing.Literal:
        return {"__values__": ["literal", *map(_repr, typing.get_args(alias))]}
    members = [paths.get(id(member), _repr(member)) for member in typing.get_args(alias)]
    return {"__members__": ["union", *members]}


def _builds_itself(cls: type, package: Package) -> bool:
    """Whether the package decides how ``cls`` is constructed: an ``__init__``
    or ``__new__`` of its own. A protocol has no constructor, and one inherited
    from the standard library is left out (it varies between Python versions)."""
    if getattr(cls, "_is_protocol", False):
        return False
    for klass in cls.__mro__:
        if "__init__" in vars(klass) or "__new__" in vars(klass):
            return package.owns(klass)
    return False


def _member_spec(value: Any) -> Spec | None:
    if isinstance(value, property | cached_property):
        return ["property"]
    if isinstance(value, staticmethod):
        return _callable_spec(value.__func__, "staticmethod", drop_first=False)
    if isinstance(value, classmethod):
        return _callable_spec(value.__func__, "classmethod", drop_first=True)
    if inspect.isfunction(value):
        return _callable_spec(value, "def", drop_first=True)
    return None


def _callable_spec(fn: Any, binding: str, *, drop_first: bool) -> Spec | None:
    try:
        signature = inspect.signature(fn)
    except (TypeError, ValueError):
        return None
    flavour = (
        "async gen "
        if inspect.isasyncgenfunction(fn)
        else "async "
        if inspect.iscoroutinefunction(fn)
        else ""
    )
    return [flavour + binding, *_tokens(signature, drop_first=drop_first)]


def _tokens(signature: inspect.Signature, *, drop_first: bool) -> Spec:
    params = list(signature.parameters.values())
    if drop_first and params and params[0].kind in (_P.POSITIONAL_ONLY, _P.POSITIONAL_OR_KEYWORD):
        params = params[1:]
    tokens: Spec = []
    keyword_only = False
    for i, p in enumerate(params):
        if p.kind is _P.VAR_POSITIONAL:
            tokens.append(f"*{p.name}")
            keyword_only = True
            continue
        if p.kind is _P.VAR_KEYWORD:
            tokens.append(f"**{p.name}")
            continue
        if p.kind is _P.KEYWORD_ONLY and not keyword_only:
            tokens.append("*")
            keyword_only = True
        tokens.append(p.name if p.default is _P.empty else f"{p.name}={_repr(p.default)}")
        following = params[i + 1].kind if i + 1 < len(params) else None
        if p.kind is _P.POSITIONAL_ONLY and following is not _P.POSITIONAL_ONLY:
            tokens.append("/")
    return tokens


def _repr(value: Any) -> str:
    if isinstance(value, set | frozenset):
        return f"{type(value).__name__}({sorted(map(repr, value))})"
    return _ADDRESS.sub("", repr(value))


def main(argv: list[str]) -> int:
    if len(argv) != 3 or argv[0] != "write":
        print("usage: api_signatures.py write PACKAGE DIRECTORY", file=sys.stderr)
        return 2
    write(Path(argv[2]), Package(argv[1]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
