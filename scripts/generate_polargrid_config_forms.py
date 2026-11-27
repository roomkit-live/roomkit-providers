#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``PolarGridConfig``, for the Gemini
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
from roomkit.providers.polargrid import PolarGridConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-polargrid/tests/fixtures/python_config_forms.json"

KEY = "placeholder-not-a-key"
POLARGRID = {"api_key": KEY}

EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("key_alone", POLARGRID),
    (
        "every_member",
        {
            **POLARGRID,
            "model": "qwen-3.6-35b-a3b",
            "region": "yul-02",
            "max_tokens": 512,
            "top_p": 0.5,
            "thinking": True,
            "timeout": 60.5,
            "connect_timeout": 2,
            "max_retries": 2,
            "debug": True,
        },
    ),
    ("an_empty_key", {**POLARGRID, "api_key": ""}),
    ("a_region_by_its_id", {**POLARGRID, "region": "sfo-03"}),
    ("a_region_by_its_name", {**POLARGRID, "region": "montreal"}),
    ("a_region_in_capitals", {**POLARGRID, "region": "YTO-01"}),
    ("no_region", {**POLARGRID, "region": None}),
    ("thinking_off", {**POLARGRID, "thinking": False}),
    ("the_least_token_bound", {**POLARGRID, "max_tokens": 1}),
    ("a_token_bound_past_the_sdk_s", {**POLARGRID, "max_tokens": 9000}),
    ("top_p_an_integer", {**POLARGRID, "top_p": 1}),
    ("members_python_does_not_have", {**POLARGRID, "base_url": "https://llm.example", "x": 1}),
]

REJECTED: list[tuple[str, Any]] = [
    ("no_key", {"model": "qwen-3.8-27b"}),
    ("key_null", {"api_key": None}),
    ("an_unknown_region", {**POLARGRID, "region": "yul-2"}),
    ("a_region_a_number", {**POLARGRID, "region": 1}),
    ("no_token_at_all", {**POLARGRID, "max_tokens": 0}),
    ("a_negative_token_bound", {**POLARGRID, "max_tokens": -5}),
    ("top_p_null", {**POLARGRID, "top_p": None}),
    ("thinking_a_text", {**POLARGRID, "thinking": "maybe"}),
    ("timeout_null", {**POLARGRID, "timeout": None}),
    ("debug_a_text", {**POLARGRID, "debug": "loud"}),
    ("a_list", [POLARGRID]),
    ("a_number", 42),
]

CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("max_tokens_a_text", {**POLARGRID, "max_tokens": "512"}),
    ("thinking_a_text_of_true", {**POLARGRID, "thinking": "true"}),
    ("thinking_one", {**POLARGRID, "thinking": 1}),
    ("timeout_a_text", {**POLARGRID, "timeout": "30"}),
]


def dumped(given: Any) -> Any:
    return PolarGridConfig.model_validate(given).model_dump(mode="json")


def refused(given: Any) -> bool:
    try:
        PolarGridConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def main() -> None:
    for name, given in REJECTED:
        if not refused(given):
            raise AssertionError(f"Python's PolarGridConfig takes the input {name}")
    fixture = {
        "generator": "scripts/generate_polargrid_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "PolarGridConfig",
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
