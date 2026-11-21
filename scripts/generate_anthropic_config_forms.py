#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``AnthropicConfig``, for the Anthropic crate's tests.

The fixture holds, each decided by Python:

- ``examples``: the keyword arguments a configuration is built from, and what
  it dumps (``model_dump(mode="json")``);
- ``rejected``: the inputs ``AnthropicConfig`` refuses;
- ``converted``: the inputs it takes by converting (a text for a number, a
  number for a switch) or without a bound (a count below zero), with what
  Python dumps of each;
- ``endpoints``: base URLs, with the base URL the configuration keeps of each
  (it drops the path of the Messages API and a trailing slash) and whether
  that is Anthropic's own endpoint, which decides the defaults of a modern
  model. A base URL that Python's URL reader refuses, the configuration with
  it, is ``refused``.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-anthropic-config``).
"""

from __future__ import annotations

import json
import platform
from pathlib import Path
from typing import Any

import roomkit
from pydantic import ValidationError
from roomkit.providers.anthropic import AnthropicConfig
from roomkit.providers.vendor_endpoint import ANTHROPIC_BASE_URL, is_vendor_endpoint

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-anthropic/tests/fixtures/python_config_forms.json"

KEY = "placeholder-not-a-key"
GATEWAY = "https://llm.example"
OLD = {"api_key": KEY, "model": "claude-sonnet-4-6"}
MODERN = {"api_key": KEY, "model": "claude-opus-5-5"}

# Each example: its name, and the keyword arguments of the configuration.
EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("required_alone", OLD),
    (
        "every_member",
        {
            "api_key": KEY,
            "model": "local-model",
            "max_tokens": 4096,
            "timeout": 120.0,
            "connect_timeout": 2.5,
            "max_retries": 3,
            "base_url": GATEWAY,
            "extra_headers": {"X-Route": "blue", "X-Tenant": "acme"},
            "enable_prompt_caching": False,
            "use_adaptive_thinking": True,
            "supports_custom_temperature": False,
        },
    ),
    # The defaults of a modern model on Anthropic's own endpoint.
    ("modern_model", MODERN),
    ("modern_model_on_the_endpoint_written_out", {**MODERN, "base_url": ANTHROPIC_BASE_URL}),
    ("modern_model_behind_a_base_url", {**MODERN, "base_url": GATEWAY}),
    (
        "modern_model_with_its_switches_stated",
        {**MODERN, "use_adaptive_thinking": False, "supports_custom_temperature": True},
    ),
    ("modern_model_with_one_switch_stated", {**MODERN, "supports_custom_temperature": True}),
    ("modern_model_with_the_other_switch_stated", {**MODERN, "use_adaptive_thinking": False}),
    (
        "old_model_with_its_switches_stated",
        {**OLD, "use_adaptive_thinking": True, "supports_custom_temperature": False},
    ),
    # Each family of names Python profiles as old, the names it profiles as
    # modern, and names that hold one without starting with it.
    *[
        (f"profile_of_{model}", {"api_key": KEY, "model": model})
        for model in (
            "claude-2.1",
            "claude-instant-1.2",
            "claude-3-opus-20240229",
            "claude-3-5-sonnet-latest",
            "claude-3-7-sonnet-20250219",
            "claude-opus-4-0",
            "claude-opus-4-1",
            "claude-opus-4-1-20250805",
            "claude-opus-4-20250514",
            "claude-sonnet-4-20250514",
            "claude-sonnet-4-5",
            "claude-sonnet-4-5-20250929",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-6",
            "claude-opus-4-6-fast",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-opus-4-10",
            "claude-opus-4-60",
            "claude-opus-4",
            "claude-opus-4-2025051",
            "claude-opus-5",
            "claude-sonnet-5-5",
            "claude-haiku-5-5",
            "claude-fable-5-2",
            "claude-mythos-5",
            "claude-next",
            "claude-",
            "claude-3",
            "claude-30",
            "claude",
            "Claude-Opus-5-5",
            "my-claude-opus-5-5",
            "anthropic.claude-opus-5-5",
            " claude-opus-5-5",
            "local-model",
        )
    ],
    # Members one by one.
    ("empty_key", {**OLD, "api_key": ""}),
    ("empty_model", {**OLD, "model": ""}),
    ("token_bound", {**OLD, "max_tokens": 64000}),
    ("token_bound_of_zero", {**OLD, "max_tokens": 0}),
    ("timeouts_as_integers", {**OLD, "timeout": 120, "connect_timeout": 2}),
    ("timeouts_with_fractions", {**OLD, "timeout": 0.25, "connect_timeout": 1.5}),
    ("timeout_of_zero", {**OLD, "timeout": 0}),
    ("retries", {**OLD, "max_retries": 2}),
    ("no_prompt_caching", {**OLD, "enable_prompt_caching": False}),
    ("headers", {**OLD, "extra_headers": {"X-Api-Key": "placeholder", "X-A": "1"}}),
    ("no_header", {**OLD, "extra_headers": {}}),
    ("base_url_null", {**MODERN, "base_url": None}),
    ("base_url_with_credentials", {**OLD, "base_url": "https://user:word@llm.example?k=word"}),
    ("members_python_does_not_have", {**OLD, "vision": True, "organization": "acme"}),
]

# The inputs the configuration refuses.
REJECTED: list[tuple[str, Any]] = [
    ("no_key", {"model": "claude-sonnet-4-6"}),
    ("no_model", {"api_key": KEY}),
    ("key_null", {**OLD, "api_key": None}),
    ("key_a_number", {**OLD, "api_key": 42}),
    ("model_null", {**OLD, "model": None}),
    ("model_a_number", {**OLD, "model": 4}),
    ("base_url_a_number", {**OLD, "base_url": 8080}),
    ("timeout_null", {**OLD, "timeout": None}),
    ("timeout_a_word", {**OLD, "timeout": "soon"}),
    ("connect_timeout_a_list", {**OLD, "connect_timeout": [5]}),
    ("max_tokens_null", {**OLD, "max_tokens": None}),
    ("max_tokens_a_word", {**OLD, "max_tokens": "many"}),
    ("max_tokens_with_a_fraction", {**OLD, "max_tokens": 10.5}),
    ("max_retries_null", {**OLD, "max_retries": None}),
    ("prompt_caching_null", {**OLD, "enable_prompt_caching": None}),
    ("switch_null", {**OLD, "use_adaptive_thinking": None}),
    ("other_switch_null", {**OLD, "supports_custom_temperature": None}),
    ("switch_a_word", {**OLD, "use_adaptive_thinking": "perhaps"}),
    ("header_a_number", {**OLD, "extra_headers": {"X-A": 1}}),
    ("headers_a_list", {**OLD, "extra_headers": [["X-A", "1"]]}),
    ("a_list", [KEY, "claude-sonnet-4-6"]),
    ("a_text", "claude-sonnet-4-6"),
    ("null", None),
]

# The inputs the configuration takes by converting, or without a bound.
CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("timeout_a_text", {**OLD, "timeout": "30"}),
    ("timeout_below_zero", {**OLD, "timeout": -1.0}),
    ("max_tokens_a_text", {**OLD, "max_tokens": "2048"}),
    ("max_tokens_a_float", {**OLD, "max_tokens": 2048.0}),
    ("max_tokens_below_zero", {**OLD, "max_tokens": -5}),
    ("max_tokens_past_32_bits", {**OLD, "max_tokens": 4294967296}),
    ("max_retries_below_zero", {**OLD, "max_retries": -1}),
    ("max_retries_a_text", {**OLD, "max_retries": "2"}),
    ("prompt_caching_a_number", {**OLD, "enable_prompt_caching": 0}),
    ("switch_a_text", {**MODERN, "use_adaptive_thinking": "false"}),
    ("switch_a_number", {**MODERN, "supports_custom_temperature": 1}),
]

ANTHROPIC = "https://api.anthropic.com"

# Base URLs, each read as the endpoint of a modern model.
ENDPOINTS: list[str] = [
    ANTHROPIC,
    f"{ANTHROPIC}/",
    f"{ANTHROPIC}//",
    "HTTPS://API.Anthropic.com",
    f"{ANTHROPIC}:443",
    f"{ANTHROPIC}:0443/",
    f"{ANTHROPIC}:00000000000000000000443",
    f"{ANTHROPIC}:",
    f"{ANTHROPIC}:444",
    f"{ANTHROPIC}:80",
    f"{ANTHROPIC}:99999",
    f"{ANTHROPIC}:99999999999999999999",
    f"{ANTHROPIC}:x",
    f"{ANTHROPIC}:443:80",
    f"{ANTHROPIC}:+443",
    "http://api.anthropic.com",
    "http://api.anthropic.com:443",
    # The path of the Messages API, which the configuration drops.
    f"{ANTHROPIC}/v1/messages",
    f"{ANTHROPIC}/v1/messages/",
    f"{ANTHROPIC}/V1/Messages",
    f"{ANTHROPIC}/v1/messages//",
    f"{ANTHROPIC}/v1/messages/v1/messages",
    f"{ANTHROPIC}/v1/messages?route=1",
    f"{ANTHROPIC}/v1/messages/count_tokens",
    f"{GATEWAY}/anthropic/v1/messages",
    "/v1/messages",
    "v1/messages",
    # Other paths.
    f"{ANTHROPIC}/v1",
    f"{ANTHROPIC}/v1/",
    f"{ANTHROPIC}/v1/..",
    f"{ANTHROPIC}/%2F",
    f"{ANTHROPIC}?route=1",
    f"{ANTHROPIC}/?route=1#top",
    f"{ANTHROPIC}#top",
    f"{ANTHROPIC}.",
    "https://api.anthropic.com.example",
    "https://example.api.anthropic.com",
    "https://user@api.anthropic.com",
    "https://user:word@api.anthropic.com",
    "https://:word@api.anthropic.com",
    "https://@api.anthropic.com",
    "https://:@api.anthropic.com",
    "https://a@b@api.anthropic.com",
    f" {ANTHROPIC} ",
    f"\t{ANTHROPIC}/\r\n",
    f"{ANTHROPIC}/ /",
    "https://api.anthro\tpic.com",
    f"\x1f{ANTHROPIC}\x1c",
    f"\x00 {ANTHROPIC}",
    f"{ANTHROPIC}\x00",
    f"\u00a0{ANTHROPIC}\u3000",
    f"{ANTHROPIC}%25x",
    "https://[api.anthropic.com]",
    "https://[::1]",
    "https://[::1",
    "https://api.anthropic.com]",
    "//api.anthropic.com",
    "api.anthropic.com",
    "https:/api.anthropic.com",
    "https:api.anthropic.com",
    "1https://api.anthropic.com",
    "ht+tp-s.x://api.anthropic.com",
    "",
    " ",
    "/",
    "https://",
    "https://\u212api.anthropic.com",
    "https://api.anthropic.com\uff0f",
    "https://caf\u00e9@api.anthropic.com",
    GATEWAY,
    f"{GATEWAY}/v1",
    "http://localhost:8080",
    "http://127.0.0.1:8000/anthropic/",
]


def form(given: Any) -> Any:
    """What the configuration built from ``given`` dumps."""
    return AnthropicConfig.model_validate(given).model_dump(mode="json")


def refuses(given: Any) -> bool:
    try:
        AnthropicConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def endpoint(base_url: str) -> dict[str, Any]:
    """The base URL the configuration keeps of ``base_url`` and whether it
    is Anthropic's own endpoint, or that Python's URL reader refuses it, and
    with it the configuration that names it."""
    try:
        dumped = form({**MODERN, "base_url": base_url})
    except ValueError:
        return {"base_url": base_url, "kept": None, "anthropic": "refused"}
    kept = dumped["base_url"]
    own = is_vendor_endpoint(kept, ANTHROPIC_BASE_URL)
    profiled = dumped["use_adaptive_thinking"], not dumped["supports_custom_temperature"]
    if profiled != (own, own):
        raise AssertionError(f"{base_url!r}: the endpoint and the profile disagree")
    return {"base_url": base_url, "kept": kept, "anthropic": own}


def main() -> None:
    for name, given in REJECTED:
        if not refuses(given):
            raise AssertionError(f"Python takes the input {name}")
    fixture = {
        "generator": "scripts/generate_anthropic_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "AnthropicConfig",
        "examples": [
            {"name": name, "given": given, "form": form(given)} for name, given in EXAMPLES
        ],
        "rejected": [{"name": name, "given": given} for name, given in REJECTED],
        "converted": [
            {"name": name, "given": given, "form": form(given)} for name, given in CONVERTED
        ],
        "endpoints": [endpoint(base_url) for base_url in ENDPOINTS],
    }
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    # Escaped, so that a no-break space or a control character shows.
    FIXTURE.write_text(json.dumps(fixture, indent=4) + "\n", encoding="utf-8")
    own = sum(found["anthropic"] is True for found in fixture["endpoints"])
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(EXAMPLES)} examples, "
        f"{len(REJECTED)} rejected, {len(CONVERTED)} converted, {len(ENDPOINTS)} base URLs "
        f"({own} Anthropic's), from roomkit {roomkit.__version__}"
    )


if __name__ == "__main__":
    main()
