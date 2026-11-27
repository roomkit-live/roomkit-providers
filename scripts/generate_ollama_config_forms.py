#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``OllamaConfig``, for the Gemini
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
from roomkit.providers.ollama import OllamaConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-ollama/tests/fixtures/python_config_forms.json"

KEY = "placeholder-not-a-key"
OLLAMA: dict[str, Any] = {}

EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("defaults", OLLAMA),
    (
        "every_member",
        {
            "host": "https://ollama.example/ollama/",
            "model": "qwen3:8b",
            "max_tokens": 512,
            "timeout": 30.5,
            "connect_timeout": 2,
            "max_retries": 3,
            "think": "high",
            "keep_alive": "10m",
            "num_ctx": 8192,
            "top_p": 0.9,
            "top_k": 40,
            "min_p": 0.05,
            "api_key": KEY,
            "headers": {"X-Proxy-Token": "t-1", "X-Team": "core"},
        },
    ),
    ("think_on", {"think": True}),
    ("think_off", {"think": False}),
    ("think_low", {"think": "low"}),
    ("keep_alive_seconds", {"keep_alive": 300}),
    ("keep_alive_text_of_seconds", {"keep_alive": "-1"}),
    ("keep_alive_text_of_seconds_in_spaces", {"keep_alive": " 0 "}),
    ("keep_alive_a_duration", {"keep_alive": "5m"}),
    ("an_empty_key", {"api_key": ""}),
    ("headers_empty", {"headers": {}}),
    ("headers_in_their_order", {"headers": {"b": "2", "a": "1"}}),
    ("probabilities_as_integers", {"top_p": 1, "min_p": 0}),
    ("members_python_does_not_have", {"base_url": "https://llm.example", "x": 1}),
]

REJECTED: list[tuple[str, Any]] = [
    ("host_null", {"host": None}),
    ("model_null", {"model": None}),
    ("max_tokens_a_float", {"max_tokens": 2.5}),
    ("think_another_level", {"think": "xhigh"}),
    ("think_a_number", {"think": 2}),
    ("keep_alive_a_float", {"keep_alive": 1.5}),
    ("keep_alive_a_list", {"keep_alive": [1]}),
    ("headers_a_list", {"headers": ["a"]}),
    ("header_a_number", {"headers": {"a": 1}}),
    ("api_key_a_number", {"api_key": 42}),
    ("timeout_null", {"timeout": None}),
    ("a_list", [OLLAMA]),
    ("a_number", 42),
]

CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("think_a_text_of_true", {"think": "true"}),
    ("think_one", {"think": 1}),
    ("max_tokens_a_text", {"max_tokens": "512"}),
    ("timeout_a_text", {"timeout": "30"}),
    ("keep_alive_a_whole_float", {"keep_alive": 300.0}),
    ("keep_alive_true", {"keep_alive": True}),
]


def dumped(given: Any) -> Any:
    return OllamaConfig.model_validate(given).model_dump(mode="json")


def refused(given: Any) -> bool:
    try:
        OllamaConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def main() -> None:
    for name, given in REJECTED:
        if not refused(given):
            raise AssertionError(f"Python's OllamaConfig takes the input {name}")
    fixture = {
        "generator": "scripts/generate_ollama_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "OllamaConfig",
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
