#!/usr/bin/env python3
"""Write the JSON forms of RoomKit's Python ``OpenAIConfig``, for the OpenAI crate's tests.

The fixture holds, each decided by Python:

- ``examples``: the keyword arguments a configuration is built from, and what
  it dumps (``model_dump(mode="json")``);
- ``rejected``: the inputs ``OpenAIConfig`` refuses;
- ``converted``: the inputs it takes by converting or without a bound, which
  the Rust type refuses, with what Python dumps of each;
- ``endpoints``: base URLs, and whether each is OpenAI's own endpoint, which
  decides the defaults of a modern model. A base URL that Python's URL reader
  refuses, the configuration with it, is ``refused``.

The Rust test ``python_config_forms`` reads and writes each with
``OpenAiConfig`` and requires the same.

Run it with a Python interpreter that has ``roomkit`` installed
(``make python-openai-config``).
"""

from __future__ import annotations

import json
import platform
from pathlib import Path
from typing import Any

import roomkit
from pydantic import ValidationError
from roomkit.providers.cerebras import CerebrasConfig
from roomkit.providers.deepseek import DeepSeekConfig
from roomkit.providers.kimi import KimiConfig
from roomkit.providers.meta import MetaConfig
from roomkit.providers.minimax import MiniMaxConfig
from roomkit.providers.openai import OpenAIConfig
from roomkit.providers.qwen import QwenConfig
from roomkit.providers.vendor_endpoint import OPENAI_BASE_URL, is_vendor_endpoint
from roomkit.providers.xai import XAIConfig

REPOSITORY = Path(__file__).resolve().parent.parent
FIXTURE = REPOSITORY / "crates/roomkit-providers-openai/tests/fixtures/python_config_forms.json"

KEY = "placeholder-not-a-key"
LOCAL = "http://localhost:11434/v1"
OLD = {"api_key": KEY, "model": "gpt-4o"}
MODERN = {"api_key": KEY, "model": "gpt-5.1"}

# Each example: its name, and the keyword arguments of the configuration.
EXAMPLES: list[tuple[str, dict[str, Any]]] = [
    ("required_alone", OLD),
    (
        "every_member",
        {
            "api_key": KEY,
            "base_url": "https://llm.example/v1",
            "model": "local-model",
            "max_tokens": 4096,
            "timeout": 120.0,
            "connect_timeout": 2.5,
            "max_retries": 3,
            "include_stream_usage": False,
            "use_max_completion_tokens": True,
            "supports_custom_temperature": False,
            "reasoning_effort": "high",
            "default_headers": {"X-Route": "blue", "X-Tenant": "acme"},
            "extra_body": {"top_k": 40, "guided_choice": ["yes", "no"], "min_p": 0.05},
            "supports_response_schema": True,
            "supports_response_schema_with_tools": False,
            "supports_reasoning_effort_with_tools": True,
            "thinking_field": "reasoning_content",
        },
    ),
    # The defaults of a modern model on OpenAI's own endpoint.
    ("modern_model", MODERN),
    ("modern_model_that_takes_a_temperature", {**OLD, "model": "gpt-4.1-mini"}),
    ("reasoning_model", {**OLD, "model": "o3"}),
    ("modern_model_on_the_endpoint_written_out", {**MODERN, "base_url": OPENAI_BASE_URL}),
    ("modern_model_behind_a_base_url", {**MODERN, "base_url": LOCAL}),
    (
        "modern_model_with_its_switches_stated",
        {**MODERN, "use_max_completion_tokens": False, "supports_custom_temperature": True},
    ),
    ("modern_model_with_one_switch_stated", {**MODERN, "supports_custom_temperature": True}),
    (
        "old_model_with_its_switches_stated",
        {**OLD, "use_max_completion_tokens": True, "supports_custom_temperature": False},
    ),
    # Each model prefix Python profiles, and names that hold one without
    # starting with it.
    *[
        (f"profile_of_{model}", {"api_key": KEY, "model": model})
        for model in (
            "gpt-4.1",
            "gpt-5",
            "gpt-6-luna",
            "o1",
            "o1-mini",
            "o3-pro",
            "o4-mini",
            "gpt-4",
            "gpt-4-turbo",
            "gpt-3.5-turbo",
            "o2",
            "ft:gpt-5:acme",
            "my-o3",
            "chatgpt-4.1",
            "GPT-5",
        )
    ],
    # Members one by one.
    ("no_token_bound", {**OLD, "max_tokens": None}),
    ("empty_key", {**OLD, "api_key": ""}),
    ("empty_model", {**OLD, "model": ""}),
    ("timeouts_as_integers", {**OLD, "timeout": 120, "connect_timeout": 2}),
    ("timeouts_with_fractions", {**OLD, "timeout": 0.25, "connect_timeout": 1.5}),
    ("timeout_of_zero", {**OLD, "timeout": 0}),
    ("retries", {**OLD, "max_retries": 2}),
    ("no_stream_usage", {**OLD, "include_stream_usage": False}),
    ("reasoning_effort", {**MODERN, "reasoning_effort": "low"}),
    ("headers", {**OLD, "default_headers": {"Authorization": "Basic placeholder", "X-A": "1"}}),
    ("no_header", {**OLD, "default_headers": {}}),
    ("base_url_with_credentials", {**OLD, "base_url": "https://user:word@llm.example/v1?k=word"}),
    ("extra_body", {**OLD, "extra_body": {"repetition_penalty": 1.1, "nested": {"b": 1, "a": 2}}}),
    ("empty_extra_body", {**OLD, "extra_body": {}}),
    ("response_schema_switches", {**OLD, "supports_response_schema": False}),
    ("thinking_field", {**OLD, "base_url": LOCAL, "thinking_field": "reasoning"}),
    ("members_python_does_not_have", {**OLD, "vision": True, "organization": "acme"}),
]

# The inputs the configuration refuses.
REJECTED: list[tuple[str, Any]] = [
    ("no_key", {"model": "gpt-4o"}),
    ("no_model", {"api_key": KEY}),
    ("key_null", {**OLD, "api_key": None}),
    ("key_a_number", {**OLD, "api_key": 42}),
    ("model_null", {**OLD, "model": None}),
    ("model_a_number", {**OLD, "model": 4}),
    ("base_url_a_number", {**OLD, "base_url": 8080}),
    ("timeout_null", {**OLD, "timeout": None}),
    ("timeout_a_word", {**OLD, "timeout": "soon"}),
    ("connect_timeout_a_list", {**OLD, "connect_timeout": [5]}),
    ("max_tokens_a_word", {**OLD, "max_tokens": "many"}),
    ("max_tokens_with_a_fraction", {**OLD, "max_tokens": 10.5}),
    ("max_retries_null", {**OLD, "max_retries": None}),
    ("stream_usage_null", {**OLD, "include_stream_usage": None}),
    ("switch_null", {**OLD, "use_max_completion_tokens": None}),
    ("other_switch_null", {**OLD, "supports_custom_temperature": None}),
    ("switch_a_word", {**OLD, "supports_response_schema": "perhaps"}),
    ("effort_a_number", {**OLD, "reasoning_effort": 3}),
    ("header_a_number", {**OLD, "default_headers": {"X-A": 1}}),
    ("headers_a_list", {**OLD, "default_headers": [["X-A", "1"]]}),
    ("extra_body_a_list", {**OLD, "extra_body": [1]}),
    ("thinking_field_a_number", {**OLD, "thinking_field": 1}),
    ("a_list", [KEY, None, "gpt-4o"]),
    ("a_text", "gpt-4o"),
    ("null", None),
]

# The inputs the configuration takes and the Rust type refuses.
CONVERTED: list[tuple[str, dict[str, Any]]] = [
    ("timeout_a_text", {**OLD, "timeout": "30"}),
    ("timeout_below_zero", {**OLD, "timeout": -1.0}),
    ("max_tokens_a_text", {**OLD, "max_tokens": "2048"}),
    ("max_tokens_a_float", {**OLD, "max_tokens": 2048.0}),
    ("max_tokens_below_zero", {**OLD, "max_tokens": -5}),
    ("max_tokens_past_32_bits", {**OLD, "max_tokens": 4294967296}),
    ("max_retries_below_zero", {**OLD, "max_retries": -1}),
    ("max_retries_a_text", {**OLD, "max_retries": "2"}),
    ("stream_usage_a_number", {**OLD, "include_stream_usage": 0}),
    ("switch_a_text", {**OLD, "use_max_completion_tokens": "true"}),
    ("switch_a_number", {**OLD, "supports_response_schema": 1}),
]

OPENAI = "https://api.openai.com"

# Base URLs, each read as the endpoint of a modern model.
ENDPOINTS: list[str] = [
    f"{OPENAI}/v1",
    f"{OPENAI}/v1/",
    f"{OPENAI}/v1//",
    "HTTPS://API.OpenAI.com/v1",
    f"{OPENAI}:443/v1",
    f"{OPENAI}:0443/v1",
    f"{OPENAI}:00000000000000000000443/v1",
    f"{OPENAI}:/v1",
    f"{OPENAI}:444/v1",
    f"{OPENAI}:80/v1",
    f"{OPENAI}:99999/v1",
    f"{OPENAI}:99999999999999999999/v1",
    f"{OPENAI}:x/v1",
    f"{OPENAI}:443:80/v1",
    f"{OPENAI}:+443/v1",
    "http://api.openai.com/v1",
    "http://api.openai.com:443/v1",
    OPENAI,
    f"{OPENAI}/",
    f"{OPENAI}/v1/chat",
    f"{OPENAI}/V1",
    f"{OPENAI}/v1/../v1",
    f"{OPENAI}/%76%31",
    f"{OPENAI}/v1?route=1",
    f"{OPENAI}/v1/?route=1#top",
    f"{OPENAI}/v1#top",
    f"{OPENAI}?route=1",
    f"{OPENAI}./v1",
    "https://api.openai.com.example/v1",
    "https://example.api.openai.com/v1",
    "https://user@api.openai.com/v1",
    "https://user:word@api.openai.com/v1",
    "https://:word@api.openai.com/v1",
    "https://@api.openai.com/v1",
    "https://:@api.openai.com/v1",
    "https://::@api.openai.com/v1",
    "https://a@b@api.openai.com/v1",
    f" {OPENAI}/v1 ",
    f"\t{OPENAI}/v1\r\n",
    "https://api.open\tai.com/v1",
    "https://api.openai.com/\nv1",
    f"\x1f{OPENAI}/v1\x1c",
    f"\x00 {OPENAI}/v1",
    f"{OPENAI}/v1\x00",
    f"\u00a0{OPENAI}/v1\u3000",
    f"\x00\u00a0{OPENAI}/v1",
    f"{OPENAI}%25x/v1",
    "https://[api.openai.com]/v1",
    "https://[::1]/v1",
    "https://[::1/v1",
    "https://api.openai.com]/v1",
    "//api.openai.com/v1",
    "api.openai.com/v1",
    "https:/api.openai.com/v1",
    "https:api.openai.com/v1",
    "1https://api.openai.com/v1",
    "ht+tp-s.x://api.openai.com/v1",
    "",
    "https://",
    "https://\u212api.openai.com/v1",
    "https://api.openai.com\uff0fv1",
    "https://caf\u00e9@api.openai.com/v1",
    "https://llm.example/v1",
    "http://localhost:11434/v1",
    "http://127.0.0.1:8000/v1",
]


def form(given: Any) -> Any:
    """What the configuration built from ``given`` dumps."""
    return OpenAIConfig.model_validate(given).model_dump(mode="json")


def refuses(given: Any) -> bool:
    try:
        OpenAIConfig.model_validate(given)
    except ValidationError:
        return True
    return False


def endpoint(base_url: str) -> dict[str, Any]:
    """Whether ``base_url`` is OpenAI's own endpoint, or refused by Python's
    URL reader, and with it the configuration that names it."""
    try:
        own: bool | str = is_vendor_endpoint(base_url, OPENAI_BASE_URL)
    except ValueError:
        own = "refused"
    try:
        dumped = form({**MODERN, "base_url": base_url})
    except ValueError:
        if own != "refused":
            raise AssertionError(f"the configuration alone refuses {base_url!r}") from None
    else:
        profiled = dumped["use_max_completion_tokens"], not dumped["supports_custom_temperature"]
        if profiled != (own, own):
            raise AssertionError(f"{base_url!r}: the endpoint and the profile disagree")
    return {"base_url": base_url, "openai": own}


def vendor_examples(
    model: str, extra: dict[str, Any], thinking_field: bool
) -> list[tuple[str, dict[str, Any]]]:
    """The examples of a vendor's configuration: its required members, every
    member, and each one apart; ``extra`` holds the vendor's own members, each
    given once, and ``thinking_field`` says whether the vendor takes one (the
    vendor whose provider sends reasoning where it reads it takes none)."""
    required = {"api_key": KEY, "model": model}
    every = {
        **required,
        "base_url": "https://llm.example/v1",
        "max_tokens": 4096,
        "timeout": 120.0,
        "connect_timeout": 2.5,
        "max_retries": 3,
        "include_stream_usage": False,
        "use_max_completion_tokens": True,
        "supports_custom_temperature": False,
        "reasoning_effort": "high",
        "default_headers": {"X-Route": "blue", "X-Tenant": "acme"},
        "extra_body": {"top_k": 40, "guided_choice": ["yes", "no"], "min_p": 0.05},
        "supports_response_schema": True,
        "supports_response_schema_with_tools": False,
        **({"thinking_field": "reasoning_content"} if thinking_field else {}),
        **{member: values[0] for member, values in extra.items()},
    }
    examples = [
        ("required_alone", required),
        ("every_member", every),
        ("behind_a_base_url", {**required, "base_url": LOCAL}),
        ("no_token_bound", {**required, "max_tokens": None}),
        ("a_token_bound", {**required, "max_tokens": 2048}),
        ("empty_key", {**required, "api_key": ""}),
        ("empty_model", {**required, "model": ""}),
        ("timeouts_as_integers", {**required, "timeout": 120, "connect_timeout": 2}),
        ("timeouts_with_fractions", {**required, "timeout": 0.25, "connect_timeout": 1.5}),
        ("retries", {**required, "max_retries": 2}),
        ("no_stream_usage", {**required, "include_stream_usage": False}),
        ("stream_usage", {**required, "include_stream_usage": True}),
        (
            "switches_stated",
            {**required, "use_max_completion_tokens": False, "supports_custom_temperature": True},
        ),
        ("reasoning_effort", {**required, "reasoning_effort": "low"}),
        (
            "headers",
            {**required, "default_headers": {"Authorization": "Basic placeholder", "X-A": "1"}},
        ),
        (
            "extra_body",
            {**required, "extra_body": {"repetition_penalty": 1.1, "nested": {"b": 1, "a": 2}}},
        ),
        ("response_schema_switches", {**required, "supports_response_schema": False}),
        ("tool_turn_setting_left_null", {**required, "supports_reasoning_effort_with_tools": None}),
        ("thinking_field_left_null", {**required, "thinking_field": None}),
        ("members_python_does_not_have", {**required, "vision": True, "organization": "acme"}),
    ]
    if thinking_field:
        examples.append(("thinking_field", {**required, "thinking_field": "reasoning"}))
    for member, values in extra.items():
        examples += [
            (f"{member}_{json.dumps(value)}", {**required, member: value}) for value in values
        ]
    return examples


def with_model(given: Any, model: str) -> Any:
    """``given`` with OpenAI's model of the examples replaced by ``model``,
    every other member as it is."""
    if isinstance(given, dict) and given.get("model") == "gpt-4o":
        return {**given, "model": model}
    return given


def vendor_rejected(
    model: str, extra_rejected: dict[str, Any], thinking_field: bool
) -> list[tuple[str, Any]]:
    """The inputs a vendor's configuration refuses: OpenAI's, the members the
    vendor's own rule makes moot (a thinking field where ``thinking_field``
    is false), and ``extra_rejected``."""
    required = {"api_key": KEY, "model": model}
    rejected = [(name, with_model(given, model)) for name, given in REJECTED]
    return [
        *rejected,
        ("tool_turn_setting_stated", {**required, "supports_reasoning_effort_with_tools": True}),
        ("tool_turn_setting_off", {**required, "supports_reasoning_effort_with_tools": False}),
        *(
            []
            if thinking_field
            else [("thinking_field_stated", {**required, "thinking_field": "reasoning_content"})]
        ),
        ("base_url_null", {**required, "base_url": None}),
        *[(name, {**required, **given}) for name, given in extra_rejected.items()],
    ]


def vendor_fixture(
    config_class: Any,
    model: str,
    extra: dict[str, Any],
    extra_rejected: dict[str, Any],
    thinking_field: bool,
) -> dict[str, Any]:
    """The fixture of a vendor's configuration, the way OpenAI's is written
    but for its base URLs, which the vendor's provider reads otherwise."""

    def dumped(given: Any) -> Any:
        return config_class.model_validate(given).model_dump(mode="json")

    def refused(given: Any) -> bool:
        try:
            config_class.model_validate(given)
        except ValidationError:
            return True
        return False

    rejected = vendor_rejected(model, extra_rejected, thinking_field)
    # A vendor whose configuration names a model of its own takes none.
    if not config_class.model_fields["model"].is_required():
        rejected = [(name, given) for name, given in rejected if name != "no_model"]
    for name, given in rejected:
        if not refused(given):
            raise AssertionError(f"Python's {config_class.__name__} takes the input {name}")
    converted = [(name, with_model(given, model)) for name, given in CONVERTED]
    return {
        "generator": "scripts/generate_openai_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": config_class.__name__,
        "examples": [
            {"name": name, "given": given, "form": dumped(given)}
            for name, given in vendor_examples(model, extra, thinking_field)
            + (
                []
                if config_class.model_fields["model"].is_required()
                else [("model_left_out", {"api_key": KEY})]
            )
        ],
        "rejected": [{"name": name, "given": given} for name, given in rejected],
        "converted": [
            {"name": name, "given": given, "form": dumped(given)} for name, given in converted
        ],
    }


# The vendors whose configuration is OpenAI's with the vendor's defaults:
# their class, a model of theirs, their own members with the values each
# example gives, the inputs of their own members Python refuses, and whether
# they take a thinking field.
VENDORS: list[tuple[str, Any, str, dict[str, Any], dict[str, Any], bool]] = [
    (
        "deepseek",
        DeepSeekConfig,
        "deepseek-v4-pro",
        {"enable_thinking": [True, False, None]},
        {"enable_thinking_a_word": {"enable_thinking": "perhaps"}},
        False,
    ),
    ("kimi", KimiConfig, "kimi-k3.1-pro", {}, {}, False),
    ("minimax", MiniMaxConfig, "MiniMax-M3", {}, {}, False),
    ("xai", XAIConfig, "grok-4.7", {}, {}, True),
    (
        "cerebras",
        CerebrasConfig,
        "gpt-oss-120b",
        {
            "reasoning_format": ["raw", "hidden", "none", "parsed", None],
            "clear_thinking": [True, False, None],
        },
        {
            "reasoning_format_unknown": {"reasoning_format": "summary"},
            "reasoning_format_a_number": {"reasoning_format": 1},
            "clear_thinking_a_word": {"clear_thinking": "perhaps"},
        },
        False,
    ),
    (
        "qwen",
        QwenConfig,
        "qwen3.7-max",
        {"enable_thinking": [True, False, None]},
        {"enable_thinking_a_word": {"enable_thinking": "perhaps"}},
        True,
    ),
    # Meta's configuration refuses a key no HTTP header can carry: a
    # placeholder's ellipsis copied as is.
    (
        "meta",
        MetaConfig,
        "muse-spark-1.3",
        {},
        {"key_not_ascii": {"api_key": "placeholder" + chr(0x2026)}},
        True,
    ),
]


def write_vendor_fixtures() -> None:
    for vendor, config_class, model, extra, extra_rejected, thinking_field in VENDORS:
        fixture = vendor_fixture(config_class, model, extra, extra_rejected, thinking_field)
        path = FIXTURE.with_name(f"python_{vendor}_config_forms.json")
        path.write_text(json.dumps(fixture, indent=4) + "\n", encoding="utf-8")
        print(
            f"Wrote {path.relative_to(REPOSITORY)}: {len(fixture['examples'])} examples, "
            f"{len(fixture['rejected'])} rejected, {len(fixture['converted'])} converted"
        )


def main() -> None:
    for name, given in REJECTED:
        if not refuses(given):
            raise AssertionError(f"Python takes the input {name}")
    fixture = {
        "generator": "scripts/generate_openai_config_forms.py",
        "roomkit_version": roomkit.__version__,
        "python_version": platform.python_version(),
        "model": "OpenAIConfig",
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
    own = sum(found["openai"] is True for found in fixture["endpoints"])
    print(
        f"Wrote {FIXTURE.relative_to(REPOSITORY)}: {len(EXAMPLES)} examples, "
        f"{len(REJECTED)} rejected, {len(CONVERTED)} converted, {len(ENDPOINTS)} base URLs "
        f"({own} OpenAI's), from roomkit {roomkit.__version__}"
    )
    write_vendor_fixtures()


if __name__ == "__main__":
    main()
