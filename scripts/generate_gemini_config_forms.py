#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``GeminiConfig``, for the Gemini
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
from roomkit.providers.gemini import GeminiConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-gemini/tests/fixtures/python_config_forms.json"

KEY = "placeholder-not-a-key"
GEMINI = {"api_key": KEY}

EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("key_alone", GEMINI),
    (
        "every_member",
        {
            **GEMINI,
            "model": "gemini-2.5-flash",
            "max_tokens": 4096,
            "thinking_level": "high",
            "timeout": 120.0,
            "connect_timeout": 2.5,
        },
    ),
    ("a_model", {**GEMINI, "model": "gemini-3.6-flash"}),
    ("empty_key", {**GEMINI, "api_key": ""}),
    ("empty_model", {**GEMINI, "model": ""}),
    ("a_token_bound", {**GEMINI, "max_tokens": 2048}),
    ("a_thinking_level", {**GEMINI, "thinking_level": "minimal"}),
    ("no_thinking_level", {**GEMINI, "thinking_level": None}),
    ("timeouts_as_integers", {**GEMINI, "timeout": 120, "connect_timeout": 2}),
    ("timeouts_with_fractions", {**GEMINI, "timeout": 0.25, "connect_timeout": 1.5}),
    ("members_python_does_not_have", {**GEMINI, "base_url": "https://llm.example", "x": 1}),
]

REJECTED: list[tuple[str, Any]] = [
    ("no_key", {"model": "gemini-3.8-flash"}),
    ("key_null", {"api_key": None}),
    ("key_a_number", {"api_key": 42}),
    ("model_null", {**GEMINI, "model": None}),
    ("model_a_number", {**GEMINI, "model": 3}),
    ("token_bound_null", {**GEMINI, "max_tokens": None}),
    ("token_bound_a_float", {**GEMINI, "max_tokens": 2.5}),
    ("thinking_level_a_number", {**GEMINI, "thinking_level": 2}),
    ("timeout_null", {**GEMINI, "timeout": None}),
    ("a_list", [GEMINI]),
    ("a_number", 42),
]

CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("timeout_a_text", {**GEMINI, "timeout": "30"}),
    ("max_tokens_a_text", {**GEMINI, "max_tokens": "2048"}),
    ("max_tokens_a_whole_float", {**GEMINI, "max_tokens": 2048.0}),
    ("connect_timeout_a_text", {**GEMINI, "connect_timeout": "5"}),
]


def dumped(given: Any) -> Any:
    return GeminiConfig.model_validate(given).model_dump(mode="json")


def refused(given: Any) -> bool:
    try:
        GeminiConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def main() -> None:
    for name, given in REJECTED:
        if not refused(given):
            raise AssertionError(f"Python's GeminiConfig takes the input {name}")
    fixture = {
        "generator": "scripts/generate_gemini_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "GeminiConfig",
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
