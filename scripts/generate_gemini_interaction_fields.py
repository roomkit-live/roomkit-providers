#!/usr/bin/env python3
"""Write how google-genai writes an interaction's steps and usage back, for the Gemini crate.

RoomKit's Python image provider keeps an interaction's steps and usage as the
SDK dumps them (``plain_metadata``, ``model_dump(mode="json", exclude_none=True)``):
a model the SDK knows writes its fields in the order it declares them, under
their Python names, then the members it does not declare, as they came; a step
or a content of a kind it does not know is written back as it came. The Rust
provider writes them the same way from this file:

- ``models``: each model reached from an interaction's ``steps`` and ``usage``,
  its fields in their declared order, each ``[name, key, holds]``: its Python
  name, the key it is read from, and what it holds (``null`` for a value kept
  as it came, ``{"model": name}``, ``{"list": holds}``, ``{"union": name}``);
- ``unions``: each union of models told apart by a member (``key``), its
  ``variants`` by that member's value; a value it does not list is kept as it
  came;
- ``steps``, ``usage``: what an interaction's steps and usage hold.

Run it with a Python interpreter that has ``google-genai`` installed.
"""

from __future__ import annotations

import functools
import json
import types
import typing
from pathlib import Path
from typing import Any

import pydantic
from google import genai
from google.genai._gaos.types import interactions

REPOSITORY = Path(__file__).resolve().parent.parent
WRITTEN = REPOSITORY / "crates/roomkit-providers-gemini/catalog/interaction_fields.json"


class Walk:
    """The models and unions reached from an interaction's members."""

    def __init__(self) -> None:
        self.models: dict[str, list[list[Any]]] = {}
        self.unions: dict[str, dict[str, Any]] = {}

    def model(self, model: type[pydantic.BaseModel]) -> dict[str, Any]:
        name = model.__name__
        if name not in self.models:
            self.models[name] = []
            fields = []
            for field_name, field in model.model_fields.items():
                key = field.alias or field_name
                fields.append([field_name, key, self.holds(field.annotation, field.metadata)])
            self.models[name] = fields
        return {"model": name}

    def holds(self, annotation: Any, metadata: list[Any] | None = None) -> Any:
        """What a field of ``annotation`` holds, as the file writes it."""
        for meta in metadata or []:
            union = self.open_union(meta)
            if union is not None:
                return union
        origin = typing.get_origin(annotation)
        if origin is typing.Annotated:
            inner, *extras = typing.get_args(annotation)
            return self.holds(inner, extras)
        if origin in (typing.Union, types.UnionType):
            arms = [arm for arm in typing.get_args(annotation) if arm is not type(None)]
            return self.holds(arms[0]) if len(arms) == 1 else None
        if origin in (list, typing.List):  # noqa: UP006
            (item,) = typing.get_args(annotation)
            return {"list": self.holds(item)}
        if isinstance(annotation, type) and issubclass(annotation, pydantic.BaseModel):
            return self.model(annotation)
        return None

    def open_union(self, meta: Any) -> dict[str, Any] | None:
        """The union a validator of the SDK tells apart by a member, if
        ``meta`` is one (``parse_open_union``)."""
        func = getattr(meta, "func", None)
        if not isinstance(func, functools.partial) or func.func.__name__ != "parse_open_union":
            return None
        name = func.keywords["union_name"]
        if name not in self.unions:
            self.unions[name] = {}
            variants = {
                value: self.model(model)["model"]
                for value, model in func.keywords["variants"].items()
            }
            self.unions[name] = {"key": func.keywords["disc_key"], "variants": variants}
        return {"union": name}


def main() -> None:
    walk = Walk()
    fields = interactions.Interaction.model_fields
    steps = walk.holds(fields["steps"].annotation, fields["steps"].metadata)
    usage = walk.holds(fields["usage"].annotation, fields["usage"].metadata)
    written = {
        "generator": "scripts/generate_gemini_interaction_fields.py",
        "google.genai_version": genai.__version__,
        "steps": steps,
        "usage": usage,
        "unions": dict(sorted(walk.unions.items())),
        "models": dict(sorted(walk.models.items())),
    }
    WRITTEN.write_text(json.dumps(written, indent=1) + "\n", encoding="utf-8")
    counted = f"{len(walk.models)} models, {len(walk.unions)} unions"
    print(f"Wrote {WRITTEN.relative_to(REPOSITORY)}: {counted}")


if __name__ == "__main__":
    main()
