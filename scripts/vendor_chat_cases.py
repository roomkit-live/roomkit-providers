"""The cases ``generate_openai_chat_cases.py`` runs on the RoomKit Python
providers of the vendors that speak OpenAI's Chat Completions with rules of
their own: DeepSeek, Kimi, MiniMax, xAI, Cerebras, Qwen, Meta, OpenRouter and
LiteLLM, each a derivative of ``OpenAIAIProvider``.

A vendor's cases show what its class redefines, on its own endpoint and
behind a base URL: its name, the tool names it takes, how a turn's sampling
and reasoning are sent, where a round's earlier reasoning goes back, how it
reports usage, which models it knows, and what it answers a failure with.
What it inherits as it is (the wire, its streams, its retries) is OpenAI's,
whose cases show it.
"""

from __future__ import annotations

from typing import Any

from openai_chat_cases import (
    CITY,
    HELLO,
    LOOKUP,
    ROUND,
    TOOLS,
    chunk,
    completion,
    failed,
    listed,
    said,
    sse,
)
from wire_capture import (
    GENERATE,
    KEY,
    LISTING,
    STREAM,
    Case,
    answer,
    registrar,
    stream,
)

PROXY = "https://llm.example/v1"

# A tool whose name holds a dot, which neither DeepSeek nor Kimi takes on its
# own endpoint.
DOTTED = {**LOOKUP, "name": "orders.lookup"}
DOTTED_TOOLS = {**HELLO, "tools": [DOTTED]}

# A round that called a tool without reasoning, before the turn's own.
ROUND_WITHOUT_REASONING = [
    {"role": "user", "content": "Where is order A-1?"},
    {
        "role": "assistant",
        "content": [
            {"type": "text", "text": "Let me look."},
            {
                "type": "tool_call",
                "id": "call_1",
                "name": "lookup_order",
                "arguments": {"number": "A-1"},
            },
        ],
    },
    {
        "role": "tool",
        "content": [
            {
                "type": "tool_result",
                "tool_call_id": "call_1",
                "name": "lookup_order",
                "result": "Shipped.",
            }
        ],
    },
]


def asked(**members: Any) -> dict[str, Any]:
    """``HELLO``, with ``members`` of the request."""
    return {**HELLO, **members}


def refusing(case: Any, refused: frozenset[str]) -> Any:
    """``case``, adding the cases named in ``refused`` with no reply: the
    vendor's provider refuses each before it sends a request."""

    def added(
        name: str,
        kinds: str,
        call: str,
        config: dict[str, Any],
        request: dict[str, Any],
        *replies: Any,
        **more: Any,
    ) -> None:
        case(name, kinds, call, config, request, *(() if name in refused else replies), **more)

    return added


def common(case: Any, config: dict[str, Any], proxy: dict[str, Any]) -> None:
    """The cases every vendor has, on ``config`` (its own endpoint) and
    ``proxy`` (a server of one's own)."""
    case("plain_answer", "plain", GENERATE, config, HELLO, answer(completion()))
    case("plain_stream", "plain stream", STREAM, config, HELLO, stream(said("Hello", " there.")))
    case(
        "temperature_of_the_request",
        "sampling",
        GENERATE,
        config,
        asked(temperature=0.2),
        answer(completion()),
    )
    case(
        "temperature_stated_by_the_configuration",
        "sampling",
        GENERATE,
        {**config, "supports_custom_temperature": True},
        asked(temperature=0.2),
        answer(completion()),
    )
    case(
        "token_bound_of_the_request",
        "sampling",
        GENERATE,
        config,
        asked(max_tokens=256),
        answer(completion()),
    )
    case(
        "token_bound_as_max_tokens",
        "sampling",
        GENERATE,
        {**config, "use_max_completion_tokens": False, "max_tokens": 512},
        HELLO,
        answer(completion()),
    )
    for effort in ("minimal", "low", "medium", "high", "xhigh", "max", "none"):
        case(
            f"reasoning_effort_{effort}",
            "reasoning",
            GENERATE,
            config,
            asked(reasoning_effort=effort),
            answer(completion()),
        )
        case(
            f"reasoning_effort_{effort}_behind_a_base_url",
            "reasoning proxy",
            GENERATE,
            proxy,
            asked(reasoning_effort=effort),
            answer(completion()),
        )
    case(
        "reasoning_effort_of_the_configuration",
        "reasoning",
        GENERATE,
        {**config, "reasoning_effort": "high"},
        HELLO,
        answer(completion()),
    )
    case(
        "reasoning_effort_of_the_request_over_the_configuration",
        "reasoning",
        GENERATE,
        {**config, "reasoning_effort": "high"},
        asked(reasoning_effort="low"),
        answer(completion()),
    )
    case(
        "thinking_turned_off",
        "reasoning",
        GENERATE,
        config,
        asked(enable_thinking=False),
        answer(completion()),
    )
    case(
        "thinking_turned_on",
        "reasoning",
        GENERATE,
        config,
        asked(enable_thinking=True),
        answer(completion()),
    )
    case(
        "thinking_budget_of_zero",
        "reasoning",
        GENERATE,
        config,
        asked(thinking_budget=0),
        answer(completion()),
    )
    case(
        "thinking_budget",
        "reasoning",
        GENERATE,
        config,
        asked(thinking_budget=2048),
        answer(completion()),
    )
    case(
        "reasoning_on_a_tool_turn",
        "reasoning tools",
        GENERATE,
        config,
        {**TOOLS, "reasoning_effort": "high"},
        answer(completion()),
    )
    case(
        "earlier_reasoning_on_a_tool_round",
        "round_reasoning tools",
        GENERATE,
        config,
        {**TOOLS, "messages": ROUND},
        answer(completion()),
    )
    case(
        "earlier_round_without_reasoning",
        "round_reasoning tools",
        GENERATE,
        config,
        {**TOOLS, "messages": ROUND_WITHOUT_REASONING},
        answer(completion()),
    )
    case(
        "earlier_reasoning_behind_a_base_url",
        "round_reasoning tools proxy",
        GENERATE,
        proxy,
        {**TOOLS, "messages": ROUND},
        answer(completion()),
    )
    case(
        "reasoning_read_from_the_answer",
        "round_reasoning",
        GENERATE,
        config,
        HELLO,
        answer(completion("Hello.", reasoning_content="The user greets me.")),
    )
    case("dotted_tool_name", "tool_names", GENERATE, config, DOTTED_TOOLS, answer(completion()))
    case(
        "dotted_tool_name_behind_a_base_url",
        "tool_names proxy",
        GENERATE,
        proxy,
        DOTTED_TOOLS,
        answer(completion()),
    )
    case(
        "response_schema",
        "response_schema",
        GENERATE,
        config,
        asked(response_schema=CITY),
        answer(completion('{"city": "Paris"}')),
    )
    case(
        "usage_with_cached_tokens",
        "usage",
        GENERATE,
        config,
        HELLO,
        answer(
            completion(
                usage={
                    "prompt_tokens": 30,
                    "completion_tokens": 5,
                    "total_tokens": 35,
                    "prompt_tokens_details": {"cached_tokens": 20},
                }
            )
        ),
    )
    case(
        "usage_with_reasoning_tokens",
        "usage",
        GENERATE,
        config,
        HELLO,
        answer(
            completion(
                usage={
                    "prompt_tokens": 12,
                    "completion_tokens": 40,
                    "total_tokens": 52,
                    "completion_tokens_details": {"reasoning_tokens": 30},
                }
            )
        ),
    )
    case(
        "status_400",
        "failure",
        GENERATE,
        config,
        HELLO,
        answer(failed("Invalid request.", "invalid_request_error"), status=400),
    )
    case(
        "status_401",
        "failure",
        GENERATE,
        config,
        HELLO,
        answer(failed("Authentication failed.", "authentication_error"), status=401),
    )
    case(
        "models_listed",
        "listing",
        LISTING,
        config,
        HELLO,
        answer(listed(config["model"])),
    )
    case(
        "a_model_the_catalogue_does_not_name",
        "catalogue",
        GENERATE,
        {**config, "model": "vendor-next"},
        HELLO,
        answer(completion()),
    )
    case(
        "a_model_behind_a_base_url", "catalogue proxy", GENERATE, proxy, HELLO, answer(completion())
    )


DEEPSEEK_CASES: list[Case] = []
_deepseek = refusing(
    registrar(DEEPSEEK_CASES),
    frozenset(
        {
            "dotted_tool_name",
            "response_schema",
            "response_schema_behind_a_base_url",
            "deepseek_s_other_base",
        }
    ),
)
DEEPSEEK = {"api_key": KEY, "model": "deepseek-v4-pro"}
common(_deepseek, DEEPSEEK, {**DEEPSEEK, "base_url": PROXY})
for _enabled in (True, False):
    _deepseek(
        f"thinking_of_the_configuration_{str(_enabled).lower()}",
        "reasoning",
        GENERATE,
        {**DEEPSEEK, "enable_thinking": _enabled},
        HELLO,
        answer(completion()),
    )
_deepseek(
    "thinking_of_the_request_over_the_configuration",
    "reasoning",
    GENERATE,
    {**DEEPSEEK, "enable_thinking": True},
    asked(enable_thinking=False),
    answer(completion()),
)
_deepseek(
    "usage_with_deepseek_s_cache_counters",
    "usage",
    GENERATE,
    DEEPSEEK,
    HELLO,
    answer(
        completion(
            usage={
                "prompt_tokens": 30,
                "completion_tokens": 5,
                "total_tokens": 35,
                "prompt_cache_hit_tokens": 20,
                "prompt_cache_miss_tokens": 10,
            }
        )
    ),
)
_deepseek(
    "usage_with_a_cache_hit_and_no_miss",
    "usage",
    GENERATE,
    DEEPSEEK,
    HELLO,
    answer(
        completion(
            usage={
                "prompt_tokens": 30,
                "completion_tokens": 5,
                "total_tokens": 35,
                "prompt_cache_hit_tokens": 20,
            }
        )
    ),
)
_deepseek(
    "usage_with_no_cache_hit",
    "usage",
    GENERATE,
    DEEPSEEK,
    HELLO,
    answer(
        completion(
            usage={
                "prompt_tokens": 30,
                "completion_tokens": 5,
                "total_tokens": 35,
                "prompt_cache_hit_tokens": 0,
                "prompt_cache_miss_tokens": 30,
            }
        )
    ),
)
_deepseek(
    "response_schema_behind_a_base_url",
    "response_schema proxy",
    GENERATE,
    {**DEEPSEEK, "base_url": PROXY},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_deepseek(
    "response_schema_stated_by_the_configuration",
    "response_schema",
    GENERATE,
    {**DEEPSEEK, "supports_response_schema": True},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_deepseek(
    "deepseek_s_other_base",
    "endpoint",
    GENERATE,
    {**DEEPSEEK, "base_url": "https://api.deepseek.com"},
    DOTTED_TOOLS,
    answer(completion()),
)
_deepseek(
    "flash_model",
    "catalogue",
    GENERATE,
    {**DEEPSEEK, "model": "deepseek-v4-flash"},
    HELLO,
    answer(completion()),
)

KIMI_CASES: list[Case] = []
_kimi = refusing(registrar(KIMI_CASES), frozenset({"dotted_tool_name"}))
KIMI = {"api_key": KEY, "model": "kimi-k3.1-pro"}
common(_kimi, KIMI, {**KIMI, "base_url": PROXY, "model": "moonshotai/Kimi-K3"})
_kimi(
    "k3_model",
    "catalogue",
    GENERATE,
    {**KIMI, "model": "kimi-k3"},
    asked(reasoning_effort="medium"),
    answer(completion()),
)
_kimi(
    "highspeed_model",
    "catalogue",
    GENERATE,
    {**KIMI, "model": "kimi-k3.1-pro-highspeed"},
    HELLO,
    answer(completion()),
)
_kimi(
    "a_catalogued_model_behind_a_base_url",
    "catalogue proxy",
    GENERATE,
    {**KIMI, "base_url": PROXY},
    asked(reasoning_effort="none"),
    answer(completion()),
)
_kimi(
    "earlier_round_without_reasoning_streamed",
    "round_reasoning tools stream",
    STREAM,
    KIMI,
    {**TOOLS, "messages": ROUND_WITHOUT_REASONING},
    stream(said("Shipped.")),
)

MINIMAX_CASES: list[Case] = []
_minimax = refusing(registrar(MINIMAX_CASES), frozenset({"response_schema"}))
MINIMAX = {"api_key": KEY, "model": "MiniMax-M3"}
common(_minimax, MINIMAX, {**MINIMAX, "base_url": PROXY, "model": "minimax-m3-local"})
FLASH = {**MINIMAX, "model": "MiniMax-M3.1-Flash"}
for _effort in ("minimal", "low", "medium", "high", "max", "none"):
    _minimax(
        f"flash_reasoning_effort_{_effort}",
        "reasoning catalogue",
        GENERATE,
        FLASH,
        asked(reasoning_effort=_effort),
        answer(completion()),
    )
_minimax(
    "flash_thinking_turned_off",
    "reasoning catalogue",
    GENERATE,
    FLASH,
    asked(enable_thinking=False),
    answer(completion()),
)
_minimax(
    "m3_off_with_extra_body",
    "reasoning",
    GENERATE,
    {**MINIMAX, "extra_body": {"thinking": {"type": "enabled"}, "top_k": 40}},
    asked(reasoning_effort="none"),
    answer(completion()),
)

# A round that reasoned, its reasoning sent back where the configuration
# names it (a vendor that takes a thinking field).
EARLIER_REASONING = {**TOOLS, "messages": ROUND}


def usage(prompt: int, completion_: int, total: int, **details: Any) -> dict[str, Any]:
    """A usage object: its counts, and the details given."""
    return {
        "prompt_tokens": prompt,
        "completion_tokens": completion_,
        "total_tokens": total,
        **details,
    }


REASONING_DETAILS = {"completion_tokens_details": {"reasoning_tokens": 30}}

XAI_CASES: list[Case] = []
_xai = registrar(XAI_CASES)
XAI = {"api_key": KEY, "model": "grok-4.7"}
common(_xai, XAI, {**XAI, "base_url": PROXY, "model": "grok-local"})
_xai(
    "model_left_out",
    "catalogue",
    GENERATE,
    {"api_key": KEY},
    asked(reasoning_effort="high"),
    answer(completion()),
)
_xai(
    "a_model_that_does_not_reason",
    "reasoning catalogue",
    GENERATE,
    {**XAI, "model": "grok-4.20-0309-non-reasoning"},
    asked(reasoning_effort="high"),
    answer(completion()),
)
_xai(
    "a_model_that_does_not_reason_on_a_tool_turn",
    "reasoning catalogue tools",
    GENERATE,
    {**XAI, "model": "grok-4.20-0309-non-reasoning", "reasoning_effort": "low"},
    TOOLS,
    answer(completion()),
)
_xai(
    "usage_with_reasoning_reported_apart",
    "usage",
    GENERATE,
    XAI,
    HELLO,
    answer(completion(usage=usage(12, 10, 52, **REASONING_DETAILS))),
)
_xai(
    "usage_with_reasoning_inside_the_completion",
    "usage",
    GENERATE,
    XAI,
    HELLO,
    answer(completion(usage=usage(12, 40, 52, **REASONING_DETAILS))),
)
_xai(
    "usage_with_reasoning_and_no_total",
    "usage",
    GENERATE,
    XAI,
    HELLO,
    answer(
        completion(
            usage={"prompt_tokens": 12, "completion_tokens": 10, **REASONING_DETAILS},
        )
    ),
)
_xai(
    "thinking_field_of_the_configuration",
    "round_reasoning tools",
    GENERATE,
    {**XAI, "thinking_field": "reasoning_content"},
    EARLIER_REASONING,
    answer(completion()),
)

CEREBRAS_CASES: list[Case] = []
_cerebras = registrar(CEREBRAS_CASES)
CEREBRAS = {"api_key": KEY, "model": "gpt-oss-120b"}
common(_cerebras, CEREBRAS, {**CEREBRAS, "base_url": PROXY, "model": "cerebras-local"})
QWEN_ON_CEREBRAS = {**CEREBRAS, "model": "qwen-3.8-27b"}
for _effort in ("none", "low", "high"):
    _cerebras(
        f"qwen_reasoning_effort_{_effort}",
        "reasoning catalogue",
        GENERATE,
        QWEN_ON_CEREBRAS,
        asked(reasoning_effort=_effort),
        answer(completion()),
    )
for _format in ("raw", "hidden", "none", None):
    _cerebras(
        f"reasoning_format_{_format or 'null'}",
        "reasoning",
        GENERATE,
        {**CEREBRAS, "reasoning_format": _format},
        HELLO,
        answer(completion()),
    )
for _clear in (True, False):
    _cerebras(
        f"clear_thinking_{str(_clear).lower()}",
        "reasoning",
        GENERATE,
        {**QWEN_ON_CEREBRAS, "clear_thinking": _clear},
        HELLO,
        answer(completion()),
    )
_cerebras(
    "stream_usage_asked_by_the_configuration",
    "stream",
    STREAM,
    {**CEREBRAS, "include_stream_usage": True},
    HELLO,
    stream(said("Hello", " there.")),
)
TAGS = {
    "name": "tag_order",
    "description": "Tag an order",
    "parameters": {
        "type": "object",
        "properties": {
            "number": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {"codes": {"type": "array", "items": {"type": "integer"}}},
                },
            },
            "note": {"type": "string"},
        },
    },
}
TAGGING = {**HELLO, "tools": [TAGS]}
for _name, _arguments in (
    ("an_array_written_as_text", '{"number": "A-1", "tags": "[\\"red\\", \\"urgent\\"]"}'),
    ("an_array_already_an_array", '{"number": "A-1", "tags": ["red"]}'),
    ("a_text_that_is_no_array", '{"number": "[1]", "note": "[not an array]"}'),
    ("an_array_whose_text_does_not_read", '{"tags": "[red, urgent"}'),
    ("a_text_that_reads_as_no_array", '{"tags": "{\\"a\\": 1}"}'),
    ("arrays_nested_as_text", '{"lines": [{"codes": "[1, 2]"}, {"codes": [3]}]}'),
    ("an_unknown_member_left_alone", '{"other": "[1, 2]"}'),
):
    _cerebras(
        f"tool_call_with_{_name}",
        "tools arguments",
        GENERATE,
        CEREBRAS,
        TAGGING,
        answer(
            completion(
                None,
                finish="tool_calls",
                tool_calls=[
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "tag_order", "arguments": _arguments},
                    }
                ],
            )
        ),
    )
_cerebras(
    "streamed_tool_call_with_an_array_written_as_text",
    "tools arguments stream",
    STREAM,
    CEREBRAS,
    TAGGING,
    stream(
        sse(
            chunk({"role": "assistant", "content": None}),
            chunk(
                {
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "tag_order",
                                "arguments": '{"tags": "[\\"red\\"]"}',
                            },
                        }
                    ]
                }
            ),
            chunk({}, finish="tool_calls"),
        )
    ),
)
_cerebras(
    "a_catalogued_model_that_sees_images",
    "catalogue",
    GENERATE,
    {**CEREBRAS, "model": "gemma-4-31b"},
    HELLO,
    answer(completion()),
)

QWEN_CASES: list[Case] = []
_qwen = refusing(
    registrar(QWEN_CASES),
    frozenset({"response_schema", "models_listed", "models_listed_behind_a_base_url"}),
)
QWEN = {"api_key": KEY, "model": "qwen3.7-max"}
common(_qwen, QWEN, {**QWEN, "base_url": PROXY, "model": "qwen-local"})
for _enabled in (True, False):
    _qwen(
        f"thinking_of_the_configuration_{str(_enabled).lower()}",
        "reasoning",
        GENERATE,
        {**QWEN, "enable_thinking": _enabled},
        HELLO,
        answer(completion()),
    )
_qwen(
    "thinking_budget_with_thinking_on",
    "reasoning",
    GENERATE,
    {**QWEN, "enable_thinking": True},
    asked(thinking_budget=512),
    answer(completion()),
)
_qwen(
    "thinking_budget_over_the_configuration_off",
    "reasoning",
    GENERATE,
    {**QWEN, "enable_thinking": False},
    asked(thinking_budget=512),
    answer(completion()),
)
_qwen(
    "thinking_of_the_request_over_the_configuration",
    "reasoning",
    GENERATE,
    {**QWEN, "enable_thinking": True},
    asked(enable_thinking=False),
    answer(completion()),
)
_qwen(
    "response_schema_stated_by_the_configuration",
    "response_schema",
    GENERATE,
    {**QWEN, "supports_response_schema": True},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_qwen(
    "models_listed_behind_a_base_url", "listing proxy", LISTING, {**QWEN, "base_url": PROXY}, HELLO
)
_qwen(
    "a_catalogued_model_that_sees_images",
    "catalogue",
    GENERATE,
    {**QWEN, "model": "qwen3.7-plus"},
    HELLO,
    answer(completion()),
)
_qwen(
    "thinking_field_of_the_configuration",
    "round_reasoning tools",
    GENERATE,
    {**QWEN, "thinking_field": "reasoning_content"},
    EARLIER_REASONING,
    answer(completion()),
)

META_CASES: list[Case] = []
_meta = registrar(META_CASES)
META = {"api_key": KEY, "model": "muse-spark-1.3"}
common(_meta, META, {**META, "base_url": PROXY, "model": "muse-local"})
_meta(
    "model_left_out",
    "catalogue",
    GENERATE,
    {"api_key": KEY},
    asked(reasoning_effort="none"),
    answer(completion()),
)
_meta(
    "chat_models_of_the_listing",
    "listing",
    LISTING,
    META,
    HELLO,
    answer(
        listed(
            "muse-spark-1.3",
            "muse-wonder-1.0",
            "muse-ember-1.0",
            "muse-image-1.0",
            "muse-voice-1.0",
            "muse-spark-next",
        )
    ),
)
_meta(
    "thinking_field_of_the_configuration",
    "round_reasoning tools",
    GENERATE,
    {**META, "thinking_field": "reasoning_content"},
    EARLIER_REASONING,
    answer(completion()),
)

OPENROUTER_CASES: list[Case] = []
_openrouter = registrar(OPENROUTER_CASES)
OPENROUTER = {"api_key": KEY, "model": "anthropic/claude-sonnet-5"}
common(_openrouter, OPENROUTER, {**OPENROUTER, "base_url": PROXY, "model": "router-local"})
for _members, _name in (
    ({"site_url": "https://app.example"}, "site_url"),
    ({"app_name": "Example App"}, "app_name"),
    ({"site_url": "https://app.example", "app_name": "Example App"}, "site_url_and_app_name"),
    (
        {"app_name": "Example App", "default_headers": {"X-Title": "Header App", "X-A": "1"}},
        "app_name_under_a_default_header",
    ),
    (
        {"app_name": "Example App", "default_headers": {"X-TITLE": "Header App"}},
        "app_name_under_a_default_header_written_in_capitals",
    ),
    ({"site_url": "", "app_name": ""}, "empty_site_url_and_app_name"),
):
    _openrouter(
        f"attribution_{_name}",
        "attribution",
        GENERATE,
        {**OPENROUTER, **_members},
        HELLO,
        answer(completion()),
    )
_openrouter(
    "thinking_turned_on_with_an_effort",
    "reasoning",
    GENERATE,
    {**OPENROUTER, "reasoning_effort": "medium"},
    asked(enable_thinking=True),
    answer(completion()),
)
_openrouter(
    "thinking_budget_over_an_effort",
    "reasoning",
    GENERATE,
    OPENROUTER,
    asked(thinking_budget=1024, reasoning_effort="high"),
    answer(completion()),
)
_openrouter(
    "response_schema_with_routing_preferences",
    "response_schema",
    GENERATE,
    {**OPENROUTER, "extra_body": {"provider": {"order": ["anthropic"]}, "top_k": 40}},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_openrouter(
    "response_schema_behind_a_base_url",
    "response_schema proxy",
    GENERATE,
    {**OPENROUTER, "base_url": PROXY, "model": "router-local"},
    asked(response_schema=CITY),
    answer(completion('{"city": "Paris"}')),
)
_openrouter(
    "reasoning_read_from_the_reasoning_member",
    "round_reasoning",
    GENERATE,
    OPENROUTER,
    HELLO,
    answer(completion("Hello.", reasoning="The user greets me.")),
)
_openrouter(
    "reasoning_streamed_in_the_reasoning_member",
    "round_reasoning stream",
    STREAM,
    OPENROUTER,
    HELLO,
    stream(
        sse(
            chunk({"role": "assistant", "content": ""}),
            chunk({"reasoning": "The user "}),
            chunk({"reasoning": "greets me."}),
            chunk({"content": "Hello."}),
            chunk({}, finish="stop"),
        )
    ),
)
_openrouter(
    "the_routed_models_listed",
    "listing",
    LISTING,
    OPENROUTER,
    HELLO,
    answer(
        {
            "data": [
                {
                    "id": "anthropic/claude-sonnet-5",
                    "name": "Anthropic: Claude Sonnet 5",
                    "context_length": 1000000,
                    "architecture": {"input_modalities": ["text", "image"]},
                },
                {
                    "id": "vendor/text-only",
                    "name": "Vendor: Text Only",
                    "context_length": 32768,
                    "architecture": {"input_modalities": ["text"]},
                },
                {"id": "vendor/unknown-modalities", "architecture": {}},
                {"id": "vendor/no-architecture", "name": None, "context_length": None},
            ]
        }
    ),
)
_openrouter(
    "a_listing_whose_data_is_no_list",
    "listing",
    LISTING,
    OPENROUTER,
    HELLO,
    answer({"data": {"id": "vendor/model"}}),
)
_openrouter(
    "a_listing_without_data",
    "listing",
    LISTING,
    OPENROUTER,
    HELLO,
    answer({"object": "list"}),
)
_openrouter(
    "thinking_field_of_the_configuration",
    "round_reasoning tools",
    GENERATE,
    {**OPENROUTER, "thinking_field": "reasoning"},
    EARLIER_REASONING,
    answer(completion()),
)

LITELLM_CASES: list[Case] = []
_litellm = registrar(LITELLM_CASES)
LITELLM = {"api_key": KEY, "model": "claude-sonnet"}
common(_litellm, LITELLM, {**LITELLM, "base_url": PROXY})
for _stated in (True, False):
    _litellm(
        f"reasoning_on_a_tool_turn_stated_{str(_stated).lower()}",
        "reasoning tools",
        GENERATE,
        {**LITELLM, "supports_reasoning_effort_with_tools": _stated},
        {**TOOLS, "reasoning_effort": "high"},
        answer(completion()),
    )
    _litellm(
        f"thinking_budget_on_a_tool_turn_stated_{str(_stated).lower()}",
        "reasoning tools",
        GENERATE,
        {**LITELLM, "supports_reasoning_effort_with_tools": _stated},
        {**TOOLS, "thinking_budget": 1024},
        answer(completion()),
    )
_litellm(
    "thinking_budget_over_an_effort",
    "reasoning",
    GENERATE,
    {**LITELLM, "reasoning_effort": "low"},
    asked(thinking_budget=1024),
    answer(completion()),
)
_litellm(
    "thinking_turned_off_over_an_effort",
    "reasoning",
    GENERATE,
    {**LITELLM, "reasoning_effort": "low"},
    asked(enable_thinking=False),
    answer(completion()),
)
_litellm(
    "thinking_budget_merged_into_the_extra_body",
    "reasoning",
    GENERATE,
    {**LITELLM, "extra_body": {"metadata": {"team": "a"}}},
    asked(thinking_budget=1024),
    answer(completion()),
)


def _deployment(name: Any, **info: Any) -> dict[str, Any]:
    """One entry of LiteLLM's ``/model/info``: a public name and what the
    proxy's cost map says of its deployment."""
    return {"model_name": name, "litellm_params": {"model": "upstream"}, "model_info": info}


_PRICED = {"input_cost_per_token": 0.000003, "output_cost_per_token": 0.000015}
_litellm(
    "the_deployments_listed",
    "listing",
    LISTING,
    LITELLM,
    HELLO,
    answer(
        {
            "data": [
                _deployment(
                    "claude-sonnet",
                    max_input_tokens=200000,
                    supports_vision=True,
                    cache_read_input_token_cost=0.0000003,
                    cache_creation_input_token_cost=0.00000375,
                    **_PRICED,
                ),
                _deployment("dictation-alias", mode="audio_transcription"),
                _deployment("narrator-alias", mode="audio_speech", supports_vision="yes"),
                _deployment("free-or-unmapped", input_cost_per_token=0, output_cost_per_token=0),
                _deployment("half-priced", input_cost_per_token=0.000001),
                _deployment(
                    "priced-by-a-boolean", input_cost_per_token=True, output_cost_per_token=0.000002
                ),
                _deployment("window-as-text", max_input_tokens="128000"),
                {"model_name": "no-model-info", "model_info": None},
                {"model_name": 42, "model_info": {}},
                {"litellm_params": {"model": "upstream"}},
            ]
        }
    ),
)
_litellm(
    "a_load_balanced_group_folded",
    "listing",
    LISTING,
    LITELLM,
    HELLO,
    answer(
        {
            "data": [
                _deployment("group", max_input_tokens=200000, supports_vision=True, **_PRICED),
                _deployment("group", max_input_tokens=128000, supports_vision=True, **_PRICED),
                _deployment("split", max_input_tokens=64000, supports_vision=False, **_PRICED),
                _deployment(
                    "split",
                    max_input_tokens=None,
                    supports_vision=True,
                    input_cost_per_token=0.000001,
                    output_cost_per_token=0.000002,
                ),
                _deployment("unsure", supports_vision=True, mode="audio_speech"),
                _deployment("unsure", mode="audio_speech"),
            ]
        }
    ),
)
_litellm(
    "a_listing_whose_data_is_no_list",
    "listing",
    LISTING,
    LITELLM,
    HELLO,
    answer({"data": {"model_name": "claude-sonnet"}}),
)
_litellm(
    "thinking_field_of_the_configuration",
    "round_reasoning tools",
    GENERATE,
    {**LITELLM, "thinking_field": "reasoning_content"},
    EARLIER_REASONING,
    answer(completion()),
)
_litellm(
    "the_proxy_s_default_base_url",
    "endpoint",
    GENERATE,
    {"api_key": KEY, "model": "claude-sonnet", "base_url": "http://localhost:4000"},
    HELLO,
    answer(completion()),
)

AZURE_CASES: list[Case] = []
_azure = registrar(AZURE_CASES)
# A deployment of an Azure resource, its model a name its operator chose.
AZURE = {"api_key": KEY, "azure_endpoint": "https://acme.openai.azure.com", "model": "chat-prod"}
# The same resource behind an API Management gateway, whose route prefix
# ends with /openai.
GATEWAY = {**AZURE, "azure_endpoint": "https://gateway.example/llm/openai"}
common(_azure, AZURE, GATEWAY)
for _endpoint, _name in (
    ("https://acme.openai.azure.com/", "a_trailing_slash"),
    ("https://acme.openai.azure.com/openai/v1", "the_v1_api_s_url"),
    ("https://acme.openai.azure.com/openai/v1/chat/completions", "the_v1_api_s_target"),
    (
        "https://acme.openai.azure.com/openai/deployments/chat-prod/chat/completions"
        "?api-version=2024-10-21",
        "a_deployment_s_target_uri",
    ),
    ("https://acme.services.ai.azure.com", "a_foundry_resource"),
    ("  https://acme.cognitiveservices.azure.com  ", "a_cognitive_services_resource_in_spaces"),
):
    _azure(
        f"endpoint_pasted_as_{_name}",
        "endpoint",
        GENERATE,
        {**AZURE, "azure_endpoint": _endpoint},
        HELLO,
        answer(completion()),
    )
_azure(
    "api_version_of_the_configuration",
    "endpoint",
    GENERATE,
    {**AZURE, "api_version": "2025-04-01-preview"},
    HELLO,
    answer(completion()),
)
for _stated in (True, False):
    _azure(
        f"reasoning_on_a_tool_turn_stated_{str(_stated).lower()}",
        "reasoning tools",
        GENERATE,
        {**AZURE, "supports_reasoning_effort_with_tools": _stated},
        {**TOOLS, "reasoning_effort": "high"},
        answer(completion()),
    )
_azure(
    "response_schema_beside_tools",
    "response_schema tools",
    GENERATE,
    AZURE,
    {**TOOLS, "response_schema": CITY},
    answer(completion('{"city": "Paris"}')),
)
_azure(
    "extra_body_of_the_configuration",
    "sampling",
    GENERATE,
    {**AZURE, "extra_body": {"data_sources": [{"type": "azure_search"}]}},
    HELLO,
    answer(completion()),
)
_azure(
    "stream_usage_refused_by_the_configuration",
    "plain stream",
    STREAM,
    {**AZURE, "include_stream_usage": False},
    HELLO,
    stream(said("Hello", " there.", usage=None)),
)
# An empty key is not captured: the openai SDK raises its own OpenAIError
# ("Missing credentials") at each call, before any request, no ProviderError,
# which no provider of the contract reproduces. The Rust provider refuses
# each call there with a ProviderError in the same words.
_azure(
    "an_endpoint_with_user_information",
    "endpoint",
    GENERATE,
    {**AZURE, "azure_endpoint": "https://user:secret@acme.openai.azure.com"},
    HELLO,
    answer(completion()),
)
_azure(
    "deployments_listed",
    "listing",
    LISTING,
    AZURE,
    HELLO,
    answer(listed("chat-prod", "embedding-prod")),
)

# The table of each vendor, by the name its fixture is written under.
VENDOR_CASES: dict[str, list[Case]] = {
    "deepseek": DEEPSEEK_CASES,
    "kimi": KIMI_CASES,
    "minimax": MINIMAX_CASES,
    "xai": XAI_CASES,
    "cerebras": CEREBRAS_CASES,
    "qwen": QWEN_CASES,
    "meta": META_CASES,
    "openrouter": OPENROUTER_CASES,
    "litellm": LITELLM_CASES,
    "azure": AZURE_CASES,
}
