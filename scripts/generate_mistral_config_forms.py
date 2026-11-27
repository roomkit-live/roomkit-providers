#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``MistralConfig``, for the Gemini
crate: what ``model_dump(mode="json")`` writes of each example, which inputs
Python refuses, and which it converts where the crate refuses them.

Run it with a Python interpreter that has ``roomkit`` installed.
"""

from __future__ import annotations

import json
import platform
from pathlib import Path
from typing import Any

import roomkit
from pydantic import ValidationError
from roomkit.providers.mistral import MistralConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-mistral/tests/fixtures/python_config_forms.json"

KEY = "placeholder-not-a-key"
MISTRAL = {"api_key": KEY}

EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("key_alone", MISTRAL),
    (
        "every_member",
        {
            **MISTRAL,
            "model": "mistral-small-latest",
            "max_tokens": 4096,
            "server_url": "https://llm.example",
            "reasoning_effort": "high",
        },
    ),
    ("a_model", {**MISTRAL, "model": "magistral-medium-latest"}),
    ("empty_key", {**MISTRAL, "api_key": ""}),
    ("empty_model", {**MISTRAL, "model": ""}),
    ("a_token_bound", {**MISTRAL, "max_tokens": 2048}),
    ("a_server_url", {**MISTRAL, "server_url": "https://gateway.example/mistral/"}),
    ("no_server_url", {**MISTRAL, "server_url": None}),
    ("a_reasoning_effort", {**MISTRAL, "reasoning_effort": "none"}),
    ("no_reasoning_effort", {**MISTRAL, "reasoning_effort": None}),
    ("members_python_does_not_have", {**MISTRAL, "base_url": "https://llm.example", "x": 1}),
]

REJECTED: list[tuple[str, Any]] = [
    ("no_key", {"model": "mistral-large-latest"}),
    ("key_null", {"api_key": None}),
    ("key_a_number", {"api_key": 42}),
    ("model_null", {**MISTRAL, "model": None}),
    ("model_a_number", {**MISTRAL, "model": 3}),
    ("token_bound_null", {**MISTRAL, "max_tokens": None}),
    ("token_bound_a_float", {**MISTRAL, "max_tokens": 2.5}),
    ("server_url_a_number", {**MISTRAL, "server_url": 8080}),
    ("reasoning_effort_a_number", {**MISTRAL, "reasoning_effort": 2}),
    ("a_list", [MISTRAL]),
    ("a_number", 42),
]

CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("max_tokens_a_text", {**MISTRAL, "max_tokens": "2048"}),
    ("max_tokens_a_whole_float", {**MISTRAL, "max_tokens": 2048.0}),
    ("max_tokens_a_boolean", {**MISTRAL, "max_tokens": True}),
]


def dumped(given: Any) -> Any:
    return MistralConfig.model_validate(given).model_dump(mode="json")


def refused(given: Any) -> bool:
    try:
        MistralConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def main() -> None:
    for name, given in REJECTED:
        if not refused(given):
            raise AssertionError(f"Python's MistralConfig takes the input {name}")
    fixture = {
        "generator": "scripts/generate_mistral_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "MistralConfig",
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
