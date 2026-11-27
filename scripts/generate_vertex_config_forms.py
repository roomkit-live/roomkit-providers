#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``GeminiVertexConfig``, for the
Gemini crate: what ``model_dump(mode="json")`` writes of each example, which
inputs Python refuses (its billing labels by Google's rules among them), and
which it converts where the crate refuses them.

A key is a placeholder text here: the configuration holds it as a secret and
reads nothing of it; the provider reads it.

Run it with a Python interpreter that has ``roomkit`` installed.
"""

from __future__ import annotations

import json
import platform
from pathlib import Path
from typing import Any

import roomkit
from pydantic import ValidationError
from roomkit.providers.gemini.vertex import GeminiVertexConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-gemini/tests/fixtures/python_vertex_config_forms.json"

VERTEX = {"project": "proj-1", "location": "northamerica-northeast1"}
KEY = "placeholder-not-a-key-file"

EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("project_and_location", VERTEX),
    (
        "every_member",
        {
            **VERTEX,
            "api_key": "placeholder-not-a-key",
            "model": "gemini-2.5-flash",
            "max_tokens": 4096,
            "thinking_level": "high",
            "timeout": 120.0,
            "connect_timeout": 2.5,
            "service_account_json": KEY,
            "impersonate_service_account": "runner@proj-1.iam.gserviceaccount.com",
            "labels": {"tenant": "acme", "équipe": "nord-1", "empty": ""},
        },
    ),
    ("a_key", {**VERTEX, "service_account_json": KEY}),
    ("labels_none", {**VERTEX, "labels": None}),
    ("labels_empty", {**VERTEX, "labels": {}}),
    ("label_of_a_caseless_script", {**VERTEX, "labels": {"東京": "大阪-1"}}),
    ("label_key_of_63", {**VERTEX, "labels": {"a" * 63: "b" * 63}}),
    ("labels_64", {**VERTEX, "labels": {f"k{n}": "v" for n in range(64)}}),
    ("global", {**VERTEX, "location": "global"}),
    ("label_of_other_digits", {**VERTEX, "labels": {"tenant": "\u0663\u0664-\u096a"}}),
    ("empty_key", {**VERTEX, "service_account_json": "", "api_key": ""}),
    ("members_python_does_not_have", {**VERTEX, "base_url": "https://llm.example", "x": 1}),
]

REJECTED: list[tuple[str, Any]] = [
    ("no_project", {"location": "global"}),
    ("no_location", {"project": "proj-1"}),
    ("project_null", {**VERTEX, "project": None}),
    ("location_a_number", {**VERTEX, "location": 3}),
    ("key_a_number", {**VERTEX, "service_account_json": 3}),
    ("labels_65", {**VERTEX, "labels": {f"k{n}": "v" for n in range(65)}}),
    ("label_key_empty", {**VERTEX, "labels": {"": "v"}}),
    ("label_key_of_64", {**VERTEX, "labels": {"a" * 64: "v"}}),
    ("label_key_starting_with_a_digit", {**VERTEX, "labels": {"1tenant": "v"}}),
    ("label_key_starting_with_an_underscore", {**VERTEX, "labels": {"_tenant": "v"}}),
    ("label_key_in_capitals", {**VERTEX, "labels": {"Tenant": "v"}}),
    ("label_key_with_a_dot", {**VERTEX, "labels": {"a.b": "v"}}),
    ("label_value_of_64", {**VERTEX, "labels": {"tenant": "v" * 64}}),
    ("label_value_in_capitals", {**VERTEX, "labels": {"tenant": "Acme"}}),
    ("label_value_with_a_space", {**VERTEX, "labels": {"tenant": "a b"}}),
    ("label_value_a_number", {**VERTEX, "labels": {"tenant": 1}}),
    ("label_key_with_a_modifier_letter", {**VERTEX, "labels": {"チーム": "v"}}),
    ("label_value_with_a_titlecase_letter", {**VERTEX, "labels": {"tenant": "ǅ"}}),
    ("labels_a_list", {**VERTEX, "labels": ["tenant"]}),
    ("a_list", [VERTEX]),
]

CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("timeout_a_text", {**VERTEX, "timeout": "30"}),
    ("max_tokens_a_text", {**VERTEX, "max_tokens": "2048"}),
]


def dumped(given: Any) -> Any:
    return GeminiVertexConfig.model_validate(given).model_dump(mode="json")


def refused(given: Any) -> bool:
    try:
        GeminiVertexConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def main() -> None:
    for name, given in REJECTED:
        if not refused(given):
            raise AssertionError(f"Python's GeminiVertexConfig takes the input {name}")
    for name, given in EXAMPLES:
        if refused(given):
            raise AssertionError(f"Python's GeminiVertexConfig refuses the example {name}")
    fixture = {
        "generator": "scripts/generate_vertex_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "GeminiVertexConfig",
        "examples": [
            {"name": name, "given": given, "form": dumped(given)} for name, given in EXAMPLES
        ],
        "rejected": [{"name": name, "given": given} for name, given in REJECTED],
        "converted": [
            {"name": name, "given": given, "form": dumped(given)} for name, given in CONVERTED
        ],
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE.write_text(json.dumps(fixture, indent=4) + "\n", encoding="utf-8")
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(EXAMPLES)} examples, "
        f"{len(REJECTED)} rejected, {len(CONVERTED)} converted"
    )


if __name__ == "__main__":
    main()
